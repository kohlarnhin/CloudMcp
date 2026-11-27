use crate::{
    executions::{self, DelegationRole, Kind, Record, Status, LOG_LIMIT},
    files, shell,
    store::{clipped, err, now, AppResult, Store},
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};

const TAIL_LIMIT: usize = 32768;
const EVENT_LIMIT: usize = 1024 * 1024;

pub fn process_identity(pid: u32) -> Option<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart=", "-o", "stat="])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields: Vec<_> = text.split_whitespace().collect();
    let state = fields.pop()?;
    let identity = fields.join(" ");
    (output.status.success() && !identity.is_empty() && !state.starts_with('Z')).then_some(identity)
}
pub fn process_matches(pid: Option<u32>, identity: Option<&str>) -> bool {
    pid.and_then(process_identity)
        .is_some_and(|current| identity.is_none_or(|expected| current == expected))
}

pub fn run(data_dir: &str, id: &str) -> AppResult<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("无效的执行编号".into());
    }
    let store = Store::open(PathBuf::from(data_dir))?;
    let pid = std::process::id();
    executions::change(&store, id, |r| {
        if r.status != Status::Starting || r.worker_pid.is_some() {
            return Err("执行记录已被其他执行器接管或不再可启动".into());
        }
        r.worker_pid = Some(pid);
        r.worker_identity = process_identity(pid);
        r.heartbeat_at = now();
        Ok(())
    })?;
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(err)?
        .block_on(drive(store.clone(), id));
    if let Err(error) = &result {
        let _ = executions::change(&store, id, |r| {
            r.status = Status::Failed;
            r.ended_at = Some(now());
            r.heartbeat_at = now();
            r.error = Some(format!("本机执行器错误：{}", clipped(error, 6000)));
            Ok(())
        });
    }
    result
}
struct ProcessGuard(Option<u32>);
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        shell::terminate(self.0);
        force_group(self.0);
    }
}
pub fn force_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

async fn drive(store: Arc<Store>, id: &str) -> AppResult<()> {
    let record = executions::load(&store, id)?;
    if record.cancel_reason.is_some() {
        executions::change(&store, id, |r| {
            r.status = Status::Cancelled;
            r.ended_at = Some(now());
            Ok(())
        })?;
        return Ok(());
    }
    let scope = store.scope(&record.scope_id)?;
    files::root(&store, &scope.id)?;
    if Path::new(&scope.path).canonicalize().map_err(err)? != Path::new(&scope.path) {
        return Err("授权目录路径已经改变，请重新授权".into());
    }
    let run_dir = store.dir.join("executions").join(id);
    let (mut command, _temp) = if record.spec.kind == Kind::Shell {
        let (cmd, temp) = shell::command(&store, &scope.id, &record.spec.command)?;
        (cmd, Some(temp))
    } else if record.spec.kind == Kind::Pi {
        (pi_command(&record, &scope.path)?, None)
    } else {
        let mut cmd = Command::new(&record.codex_path);
        cmd.arg("exec");
        if let Some(session) = &record.session_id {
            cmd.arg("resume").arg(session);
        }
        cmd.args(["--json", "--skip-git-repo-check"]);
        if record.delegation == Some(DelegationRole::Review) {
            cmd.args([
                "-c",
                "approval_policy=\"never\"",
                "-c",
                "sandbox_mode=\"read-only\"",
            ]);
        } else {
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
        }
        if record.delegation.is_some() {
            cmd.args([
                "-c",
                "shell_environment_policy.set.CODEX_DELEGATE_WORKER=\"1\"",
            ])
            .env("CODEX_DELEGATE_WORKER", "1");
        }
        cmd.arg("--output-last-message")
            .arg(run_dir.join("result.txt"))
            .arg("-")
            .current_dir(&scope.path);
        if let Some(home) = &record.codex_home {
            cmd.env("CODEX_HOME", home);
        }
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut paths: Vec<_> = std::env::split_paths(&inherited).collect();
        if let Some(parent) = Path::new(&record.codex_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            paths.push(parent.to_path_buf());
        }
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".local/bin"));
        }
        paths.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ]
            .map(PathBuf::from),
        );
        cmd.env("PATH", std::env::join_paths(paths).map_err(err)?);
        (cmd, None)
    };
    command
        .stdin(if record.spec.kind == Kind::Codex {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(err)?;
    let pid = child.id();
    let _guard = ProcessGuard(pid);
    executions::change(&store, id, |r| {
        r.status = Status::Running;
        r.child_pid = pid;
        r.child_identity = pid.and_then(process_identity);
        r.heartbeat_at = now();
        Ok(())
    })?;
    let out_state = Arc::new(Mutex::new(Capture::default()));
    let agent = record.spec.kind.agent();
    let err_state = Arc::new(Mutex::new(Capture::default()));
    let mut out = tokio::spawn(capture(
        child.stdout.take().unwrap(),
        run_dir.join("stdout.log"),
        out_state.clone(),
        agent.then(|| (store.clone(), id.to_owned())),
    ));
    let mut stderr = tokio::spawn(capture(
        child.stderr.take().unwrap(),
        run_dir.join("stderr.log"),
        err_state.clone(),
        None,
    ));
    let input = if let Some(mut stdin) = child.stdin.take() {
        let mut prompt = match record.delegation {
            Some(DelegationRole::Implement) => {
                include_str!("../../skills/codex-delegate/assets/implement.md").to_owned()
            }
            Some(DelegationRole::Review) => {
                include_str!("../../skills/codex-delegate/assets/review.md").to_owned()
            }
            None => String::new(),
        };
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str(&record.spec.prompt);
        if !record.spec.acceptance_criteria.is_empty() {
            prompt.push_str(&format!(
                "\n\n验收条件：\n{}",
                record.spec.acceptance_criteria
            ));
        }
        if record.delegation.is_none() {
            prompt.push_str("\n\n请在最终结果中说明改动、实际执行的验证和未完成项；未执行的验证请明确说明。截图、测试报告等证据请保存在本工作目录内，并返回相对路径，供远端按需读取。");
        }
        Some(tokio::spawn(async move {
            match stdin.write_all(prompt.as_bytes()).await {
                Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(err(error)),
                _ => Ok(()),
            }
        }))
    } else {
        None
    };
    let started = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    let mut last_heartbeat = Instant::now() - Duration::from_secs(3);
    let terminate_signal = termination();
    tokio::pin!(terminate_signal);
    let (exit, forced) = loop {
        tokio::select! {
            result=child.wait()=>break (result.map_err(err)?,None),
            _=interval.tick()=>{
                let current=executions::load(&store,id)?;
                let reason=if current.cancel_reason.is_some() || store.scope(&record.scope_id).is_err(){Some(Status::Cancelled)}
                    else if started.elapsed().as_secs()>=record.timeout_seconds{Some(Status::TimedOut)}else{None};
                if let Some(reason)=reason {break (stop_child(&mut child,pid).await?,Some(reason));}
                if last_heartbeat.elapsed()>=Duration::from_secs(2){
                    executions::change(&store,id,|r|{r.heartbeat_at=now();snapshot(r,&out_state,&err_state);Ok(())})?;
                    last_heartbeat=Instant::now();
                }
            }
            _=&mut terminate_signal=>{
                executions::change(&store,id,|r|{r.cancel_reason=Some("本机执行器收到停止信号".into());Ok(())})?;
                break (stop_child(&mut child,pid).await?,Some(Status::Cancelled));
            }
        }
    };
    shell::terminate(pid);
    tokio::time::sleep(Duration::from_millis(150)).await;
    force_group(pid);
    let out_result = tokio::time::timeout(Duration::from_secs(3), &mut out).await;
    let err_result = tokio::time::timeout(Duration::from_secs(3), &mut stderr).await;
    let mut capture_error = None;
    if let Some(mut input) = input {
        match tokio::time::timeout(Duration::from_secs(1), &mut input).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => capture_error = Some(error),
            other => {
                capture_error = Some(format!("输入写入未完成：{other:?}"));
                input.abort();
            }
        }
    }
    match out_result {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => capture_error = Some(e),
        other => {
            capture_error = Some(format!("stdout 捕获未完成：{other:?}"));
            out.abort();
        }
    }
    match err_result {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => capture_error = Some(e),
        other => {
            capture_error = Some(format!("stderr 捕获未完成：{other:?}"));
            stderr.abort();
        }
    }
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        exit.signal()
    };
    #[cfg(not(unix))]
    let signal = None;
    let final_text = read_final(run_dir.join("result.txt")).await;
    let final_record = executions::change(&store, id, |r| {
        snapshot(r, &out_state, &err_state);
        r.status = forced.unwrap_or(
            if exit.success() && !r.turn_failed && capture_error.is_none() {
                Status::Succeeded
            } else {
                Status::Failed
            },
        );
        r.exit_code = exit.code();
        r.signal = signal;
        r.ended_at = Some(now());
        r.heartbeat_at = now();
        if let Some(text) = final_text {
            r.summary = text;
        } else if r.spec.kind == Kind::Shell {
            r.summary = clipped(&r.stdout_tail, 6000);
        }
        if let Some(error) = capture_error {
            r.error = Some(error);
        }
        if r.status == Status::Succeeded {
            r.error = None;
            if r.spec.kind == Kind::Codex && r.session_id.is_none() {
                r.status = Status::Failed;
                r.error=Some("Codex 进程已结束，但没有收到有效的会话 ID；请检查输出与 CLI 版本，勿直接创建新会话重试".into());
            } else if r.spec.kind == Kind::Pi && !r.turn_completed {
                r.status = Status::Failed;
                r.error = Some("Pi 进程已结束，但没有收到 agent_settled 事件；请读取 stdout/stderr，并确认 Pi 版本支持 --mode json".into());
            }
        } else if r.status == Status::TimedOut {
            r.error = Some(format!(
                "达到本机执行时限 {} 秒；已有文件改动保留",
                r.timeout_seconds
            ));
        } else if r.status == Status::Cancelled {
            r.error = r.cancel_reason.clone();
        } else if r.error.is_none() {
            // Pi reports setup problems such as a missing API key on stderr before any event.
            let reason = r.stderr_tail.lines().map(str::trim).find(|l| !l.is_empty());
            r.error = Some(match (r.spec.kind, reason) {
                (Kind::Pi, Some(line)) => {
                    format!("Pi 退出码 {:?}：{}", exit.code(), clipped(line, 600))
                }
                _ => format!("进程退出码 {:?}；请按需读取 stderr", exit.code()),
            });
        }
        Ok(())
    })?;
    store.audit(
        "execution.end",
        &scope.name,
        final_record.status == Status::Succeeded,
    );
    Ok(())
}

async fn termination() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
            return;
        }
    }
    std::future::pending::<()>().await;
}

async fn stop_child(child: &mut Child, pid: Option<u32>) -> AppResult<std::process::ExitStatus> {
    shell::terminate(pid);
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(result) => result.map_err(err),
        Err(_) => {
            #[cfg(unix)]
            if let Some(pid) = pid {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            child.wait().await.map_err(err)
        }
    }
}
#[derive(Default)]
struct Capture {
    tail: Vec<u8>,
    bytes: u64,
}
fn snapshot(r: &mut Record, out: &Mutex<Capture>, stderr: &Mutex<Capture>) {
    let out = out.lock().unwrap();
    let stderr = stderr.lock().unwrap();
    r.stdout_tail = String::from_utf8_lossy(&out.tail).into_owned();
    r.stderr_tail = String::from_utf8_lossy(&stderr.tail).into_owned();
    r.stdout_bytes = out.bytes;
    r.stderr_bytes = stderr.bytes;
}
async fn capture(
    mut reader: impl AsyncRead + Unpin,
    path: PathBuf,
    state: Arc<Mutex<Capture>>,
    events: Option<(Arc<Store>, String)>,
) -> AppResult<()> {
    let mut file = tokio::fs::File::create(path).await.map_err(err)?;
    let mut saved = 0u64;
    let mut chunk = [0u8; 8192];
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let n = reader.read(&mut chunk).await.map_err(err)?;
        if n == 0 {
            break;
        }
        let keep = (LOG_LIMIT.saturating_sub(saved) as usize).min(n);
        if keep > 0 {
            file.write_all(&chunk[..keep]).await.map_err(err)?;
            saved += keep as u64;
        }
        {
            let mut state = state.lock().unwrap();
            state.bytes += n as u64;
            state.tail.extend_from_slice(&chunk[..n]);
            if state.tail.len() > TAIL_LIMIT {
                let excess = state.tail.len() - TAIL_LIMIT;
                state.tail.drain(..excess);
            }
        }
        if let Some((store, id)) = &events {
            for byte in &chunk[..n] {
                if *byte == b'\n' {
                    if !oversized {
                        event(store, id, &line)?;
                    }
                    line.clear();
                    oversized = false;
                } else if !oversized {
                    if line.len() < EVENT_LIMIT {
                        line.push(*byte);
                    } else {
                        line.clear();
                        oversized = true;
                    }
                }
            }
        }
    }
    if let Some((store, id)) = &events {
        if !line.is_empty() && !oversized {
            event(store, id, &line)?;
        }
    }
    file.sync_all().await.map_err(err)?;
    Ok(())
}
fn event(store: &Store, id: &str, line: &[u8]) -> AppResult<()> {
    let Ok(event) = serde_json::from_slice::<Value>(line) else {
        return Ok(());
    };
    match event["type"].as_str().unwrap_or_default() {
        "thread.started" => {
            if let Some(session) = event["thread_id"]
                .as_str()
                .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            {
                executions::change(store, id, |r| {
                    r.session_id = Some(session.to_owned());
                    Ok(())
                })?;
            }
        }
        "turn.completed" => {
            executions::change(store, id, |r| {
                r.turn_completed = true;
                r.turn_failed = false;
                Ok(())
            })?;
        }
        "turn.failed" | "error" => {
            let message = event["error"]["message"]
                .as_str()
                .or_else(|| event["message"].as_str())
                .unwrap_or("Codex 报告执行错误");
            executions::change(store, id, |r| {
                r.error = Some(clipped(message, 6000));
                if event["type"] == "turn.failed" {
                    r.turn_failed = true;
                }
                Ok(())
            })?;
        }
        // Pi JSON mode: the session header, assistant messages and the settled marker.
        "session" => {
            if let Some(session) = event["id"].as_str() {
                executions::change(store, id, |r| {
                    if r.spec.kind == Kind::Pi && r.session_id.as_deref() != Some(session) {
                        r.session_id = Some(session.to_owned());
                    }
                    Ok(())
                })?;
            }
        }
        "message_end" if event["message"]["role"] == "assistant" => {
            let message = &event["message"];
            let text = pi_text(message);
            let stop = message["stopReason"].as_str().unwrap_or_default();
            executions::change(store, id, |r| {
                if !text.trim().is_empty() {
                    r.summary = clipped(text.trim(), 16000);
                }
                if matches!(stop, "error" | "aborted") {
                    r.turn_failed = true;
                    let detail = message["errorMessage"]
                        .as_str()
                        .unwrap_or("Pi 模型调用失败");
                    r.error = Some(clipped(detail, 6000));
                } else if !stop.is_empty() {
                    r.turn_failed = false;
                    r.error = None;
                }
                Ok(())
            })?;
        }
        "auto_retry_end" if event["success"] == false => {
            let detail = event["finalError"]
                .as_str()
                .unwrap_or("Pi 重试后仍失败")
                .to_owned();
            executions::change(store, id, |r| {
                r.turn_failed = true;
                r.error = Some(clipped(&detail, 6000));
                Ok(())
            })?;
        }
        "agent_settled" => {
            let aborted = event["aborted"] == true;
            executions::change(store, id, |r| {
                r.turn_completed = true;
                if aborted {
                    r.turn_failed = true;
                }
                Ok(())
            })?;
        }
        "item.completed" if event["item"]["type"] == "agent_message" => {
            if let Some(text) = event["item"]["text"].as_str() {
                executions::change(store, id, |r| {
                    r.summary = clipped(text, 16000);
                    Ok(())
                })?;
            }
        }
        _ => {}
    }
    Ok(())
}
/// The prompt Pi receives: the caller's request, its acceptance criteria and the reporting rules.
/// It always starts with plain text so Pi never reads it as a flag or an `@file` reference.
pub fn pi_prompt(record: &Record) -> String {
    let mut prompt = String::from("任务：\n");
    prompt.push_str(&record.spec.prompt);
    if !record.spec.acceptance_criteria.is_empty() {
        prompt.push_str(&format!(
            "\n\n验收条件：\n{}",
            record.spec.acceptance_criteria
        ));
    }
    prompt.push_str("\n\n请在最终结果中说明改动、实际执行的验证和未完成项；未执行的验证请明确说明。截图、测试报告等证据请保存在本工作目录内，并返回相对路径，供远端按需读取。");
    prompt
}

/// Pi runs outside the command sandbox with the user's own HOME, so its models, keys, settings,
/// MCP servers and codemode from ~/.pi apply. CloudMCP chooses the session ID up front.
fn pi_command(record: &Record, workspace: &str) -> AppResult<Command> {
    let session = record.session_id.as_deref().ok_or("Pi 执行缺少会话 ID")?;
    let mut cmd = Command::new(&record.codex_path);
    cmd.args(["--mode", "json", "--session-id", session])
        .arg(pi_prompt(record))
        .current_dir(workspace);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<_> = std::env::split_paths(&inherited).collect();
    if let Some(parent) = Path::new(&record.codex_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        paths.insert(0, parent.to_path_buf());
    }
    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".local/bin",
            ".pi/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            ".cargo/bin",
        ] {
            paths.push(home.join(rel));
        }
    }
    paths.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .map(PathBuf::from),
    );
    cmd.env("PATH", std::env::join_paths(paths).map_err(err)?);
    Ok(cmd)
}

/// Text blocks of a Pi assistant message, joined in order.
fn pi_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

async fn read_final(path: PathBuf) -> Option<String> {
    let file = tokio::fs::File::open(path).await.ok()?;
    let mut bytes = Vec::new();
    let mut limited = file.take(65536);
    limited.read_to_end(&mut bytes).await.ok()?;
    Some(clipped(String::from_utf8_lossy(&bytes).trim(), 16000))
}

#[cfg(test)]
mod pi_tests {
    use super::{event, pi_text};
    use crate::{executions, store::Store};
    use serde_json::json;

    #[test]
    fn pi_text_joins_text_blocks_only() {
        let message = json!({"role":"assistant","content":[
            {"type":"thinking","thinking":"hidden"},
            {"type":"text","text":"第一段"},
            {"type":"toolCall","id":"x"},
            {"type":"text","text":"第二段"}]});
        assert_eq!(pi_text(&message), "第一段\n第二段");
        assert_eq!(pi_text(&json!({"content":"plain"})), "plain");
    }

    #[tokio::test]
    async fn pi_events_update_summary_and_completion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("w");
        std::fs::create_dir(&workspace).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        store.add_scope(&workspace, Some("w")).unwrap();
        let mut settings = store.config();
        settings.local_agent = "pi".into();
        settings.pi_path = "/nonexistent/pi".into();
        store.save_settings(settings).unwrap();
        let started = executions::pi(
            store.clone(),
            serde_json::from_value(
                json!({"workspace":"w","request_id":"r1","prompt":"改一下","wait_seconds":0}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let run_id = started["run_id"].as_str().unwrap().to_owned();
        let session = started["session_id"].as_str().unwrap().to_owned();
        assert!(uuid::Uuid::parse_str(&session).is_ok());

        let line = |v: serde_json::Value| serde_json::to_vec(&v).unwrap();
        event(&store, &run_id, &line(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"已完成"}],"stopReason":"stop"}}))).unwrap();
        event(
            &store,
            &run_id,
            &line(json!({"type":"agent_settled","aborted":false})),
        )
        .unwrap();
        let record = executions::load(&store, &run_id).unwrap();
        assert_eq!(record.summary, "已完成");
        assert!(record.turn_completed && !record.turn_failed);

        event(&store, &run_id, &line(json!({"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"401 invalid key"}}))).unwrap();
        let record = executions::load(&store, &run_id).unwrap();
        assert!(record.turn_failed);
        assert_eq!(record.error.as_deref(), Some("401 invalid key"));

        let resumed = executions::pi(
            store.clone(),
            serde_json::from_value(json!({"workspace":"w","request_id":"r2","prompt":"继续","session_id":"unknown-session","wait_seconds":0})).unwrap(),
        )
        .await;
        assert!(resumed.unwrap_err().contains("已登记"));
    }
}
