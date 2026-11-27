use crate::{
    files, privacy,
    store::{clipped, err, now, AppResult, Scope, Store},
    worker,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const LOG_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
    Interrupted,
}
impl Status {
    pub fn active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Unknown)
    }
    fn key(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Unknown => "unknown",
            Self::Interrupted => "interrupted",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Shell,
    Codex,
}
impl Kind {
    fn key(self) -> &'static str {
        if self == Self::Codex {
            "codex"
        } else {
            "shell"
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DelegationRole {
    Implement,
    Review,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Specification {
    pub kind: Kind,
    pub command: String,
    pub prompt: String,
    pub acceptance_criteria: String,
    pub session_id: Option<String>,
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<DelegationRole>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub run_id: String,
    pub scope_id: String,
    pub workspace: String,
    pub request_id: String,
    pub spec: Specification,
    /// Effective role, which can be inherited when resuming a session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<DelegationRole>,
    pub status: Status,
    pub session_id: Option<String>,
    pub codex_home: Option<String>,
    pub codex_path: String,
    pub timeout_seconds: u64,
    pub created_at: String,
    pub updated_at: String,
    pub heartbeat_at: String,
    pub ended_at: Option<String>,
    pub worker_pid: Option<u32>,
    pub worker_identity: Option<String>,
    pub child_pid: Option<u32>,
    pub child_identity: Option<String>,
    pub cancel_reason: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub error: Option<String>,
    pub turn_completed: bool,
    pub turn_failed: bool,
    pub summary: String,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    /// Records written by the task version lack the acceptance fields; read them as unverified.
    #[serde(default = "unverified")]
    pub verification: String,
    #[serde(default)]
    pub verification_note: String,
    #[serde(default)]
    pub evidence_paths: Vec<String>,
}
fn unverified() -> String {
    "unverified".into()
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CommandArgs {
    /// Authorized directory name returned by workspace_list.
    #[serde(alias = "scope_id")]
    pub workspace: String,
    pub command: String,
    /// Reuse the same ID when retrying the same request, including after changing agents.
    pub request_id: Option<String>,
    /// Actual process limit, 1–86400 seconds; defaults to the local setting.
    pub timeout_seconds: Option<u64>,
    /// How long this HTTP call waits, 0–10 seconds. The process continues afterwards.
    pub wait_seconds: Option<u64>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct CodexArgs {
    pub workspace: String,
    pub request_id: String,
    pub prompt: String,
    #[serde(default)]
    pub acceptance_criteria: String,
    /// Omit for a new session; specify a registered ID to resume. Never use --last.
    pub session_id: Option<String>,
    /// Set implement for a delegated implementation or review for read-only review.
    /// When omitted on resume, retain the previous round's role.
    pub delegation: Option<DelegationRole>,
    pub timeout_seconds: Option<u64>,
    pub wait_seconds: Option<u64>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    pub workspace: String,
    pub run_id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct WaitArgs {
    pub workspace: String,
    pub run_id: String,
    /// Seconds to wait for the execution to stop running, 0–50 (default 30).
    pub timeout_seconds: Option<u64>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    pub workspace: String,
    pub kind: Option<Kind>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct OutputArgs {
    pub workspace: String,
    pub run_id: String,
    /// stdout, stderr, prompt, acceptance_criteria, summary, or worker (runner diagnostics).
    pub stream: String,
    pub offset: Option<u64>,
    pub max_bytes: Option<usize>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ReviewArgs {
    pub workspace: String,
    pub run_id: String,
    /// unverified, completed, or needs_work. Completion requires evidence in note.
    pub verification: String,
    pub note: String,
    #[serde(default)]
    pub evidence_paths: Vec<String>,
    /// For an unknown run only: explicitly confirm that its processes have stopped after inspection.
    #[serde(default)]
    pub confirm_stopped: bool,
}

pub fn init(db: &Connection) -> AppResult<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS execution_records (
        run_id TEXT PRIMARY KEY, scope_id TEXT NOT NULL, request_id TEXT NOT NULL,
        kind TEXT NOT NULL, status TEXT NOT NULL, session_id TEXT, created_at TEXT NOT NULL,
        payload TEXT NOT NULL, UNIQUE(scope_id,request_id));
        CREATE INDEX IF NOT EXISTS execution_workspace ON execution_records(scope_id,created_at DESC);
        CREATE UNIQUE INDEX IF NOT EXISTS execution_active_workspace ON execution_records(scope_id)
            WHERE kind='codex' AND status IN ('starting','running','unknown');
        CREATE UNIQUE INDEX IF NOT EXISTS execution_active_session ON execution_records(session_id)
            WHERE kind='codex' AND session_id IS NOT NULL AND status IN ('starting','running','unknown');").map_err(err)
}
pub fn load(store: &Store, id: &str) -> AppResult<Record> {
    let raw: String = store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT payload FROM execution_records WHERE run_id=?",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| "找不到执行记录".to_owned())?;
    serde_json::from_str(&raw).map_err(err)
}
pub fn change(
    store: &Store,
    id: &str,
    f: impl FnOnce(&mut Record) -> AppResult<()>,
) -> AppResult<Record> {
    let mut db = store.db.lock().unwrap();
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    let raw: String = tx
        .query_row(
            "SELECT payload FROM execution_records WHERE run_id=?",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut record: Record = serde_json::from_str(&raw).map_err(err)?;
    f(&mut record)?;
    record.updated_at = now();
    tx.execute(
        "UPDATE execution_records SET status=?,session_id=?,payload=? WHERE run_id=?",
        params![
            record.status.key(),
            record.session_id,
            serde_json::to_string(&record).map_err(err)?,
            id
        ],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(record)
}
fn records(
    store: &Store,
    scope: Option<&str>,
    kind: Option<Kind>,
    offset: usize,
    limit: usize,
) -> AppResult<Vec<Record>> {
    let db = store.db.lock().unwrap();
    let mut q = db.prepare("SELECT payload FROM execution_records WHERE (?1 IS NULL OR scope_id=?1) AND (?2 IS NULL OR kind=?2) ORDER BY created_at DESC,run_id DESC LIMIT ?3 OFFSET ?4").map_err(err)?;
    let rows = q
        .query_map(params![scope, kind.map(Kind::key), limit, offset], |r| {
            r.get::<_, String>(0)
        })
        .map_err(err)?;
    rows.map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
        .collect()
}
fn elapsed(timestamp: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds())
        .unwrap_or(i64::MAX)
}
fn reconcile(store: &Store, record: Record) -> AppResult<Record> {
    if !matches!(record.status, Status::Starting | Status::Running)
        || elapsed(&record.heartbeat_at) < 20
    {
        return Ok(record);
    }
    if worker::process_matches(record.worker_pid, record.worker_identity.as_deref()) {
        return Ok(record);
    }
    change(store, &record.run_id, |r| {
        if matches!(r.status, Status::Starting | Status::Running) && elapsed(&r.heartbeat_at) >= 20
        {
            r.status = Status::Unknown;
            r.error = Some("本机执行器失联；请核实进程、文件改动与日志。无法据此判断远端是否额度不足，请勿重复启动。".into());
        }
        Ok(())
    })
}
fn authorized(store: &Store, workspace: &str, id: &str) -> AppResult<(Scope, Record)> {
    let scope = store.scope(workspace)?;
    let record = load(store, id)?;
    if record.scope_id != scope.id {
        return Err("执行记录不属于此授权目录".into());
    }
    Ok((scope, reconcile(store, record)?))
}
pub fn view(store: &Store, scope: &Scope, r: &Record, detail: bool) -> Value {
    let cap = store.config().result_chars;
    let mut result = json!({
        "run_id":r.run_id,"workspace":scope.name,"request_id":r.request_id,"kind":r.spec.kind,
        "status":r.status,"session_id":r.session_id,"verification":r.verification,
        "created_at":r.created_at,"updated_at":r.updated_at,"heartbeat_at":r.heartbeat_at,"ended_at":r.ended_at,
        "timeout_seconds":r.timeout_seconds,"success":if r.status.active(){None}else{Some(r.status==Status::Succeeded)},
        "exit_code":r.exit_code,"signal":r.signal,"timed_out":r.status==Status::TimedOut,
        "error":r.error.as_ref().map(|s|clipped(&privacy::text(s),cap)),
        "summary":clipped(&privacy::text(&r.summary),if detail{cap}else{600}),
        "request_brief":clipped(&privacy::text(if r.spec.kind==Kind::Codex{&r.spec.prompt}else{&r.spec.command}),200),
        "acceptance_criteria":clipped(&privacy::text(&r.spec.acceptance_criteria),if detail{cap}else{600}),
        "verification_note":clipped(&privacy::text(&r.verification_note),if detail{cap}else{600}),
        "evidence_paths":r.evidence_paths,
        "full_access":r.spec.kind==Kind::Codex && r.delegation!=Some(DelegationRole::Review),
        "can_resume":r.spec.kind==Kind::Codex && r.session_id.is_some() && !r.status.active(),
    });
    if let Some(role) = r.delegation {
        result["delegation"] = json!(role);
    }
    if detail {
        result["command"] = json!(clipped(&privacy::text(&r.spec.command), cap));
        result["prompt"] = json!(clipped(&privacy::text(&r.spec.prompt), cap / 2));
        result["stdout"] = json!(clipped(&privacy::text(&r.stdout_tail), cap / 2));
        result["stderr"] = json!(clipped(&privacy::text(&r.stderr_tail), cap / 2));
        result["output_view"] = json!("bounded tail; read stored logs with execution_output");
        result["stdout_bytes"] = json!(r.stdout_bytes);
        result["stderr_bytes"] = json!(r.stderr_bytes);
        result["log_limit_bytes_per_stream"] = json!(LOG_LIMIT);
        result["logs_truncated"] = json!(r.stdout_bytes > LOG_LIMIT || r.stderr_bytes > LOG_LIMIT);
        result["cancel_requested"] = json!(r.cancel_reason.is_some());
        result["worker_pid"] = json!(r.worker_pid);
        result["child_pid"] = json!(r.child_pid);
    }
    result
}
pub fn list(store: &Store, p: ListArgs) -> AppResult<Value> {
    let scope = store.scope(&p.workspace)?;
    let limit = p.limit.unwrap_or(20).clamp(1, 50);
    let offset = p.offset.unwrap_or(0);
    if offset > 100000 {
        return Err("分页超出上限".into());
    }
    let mut rows = records(store, Some(&scope.id), p.kind, offset, limit + 1)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let entries: Vec<_> = rows
        .into_iter()
        .map(|r| reconcile(store, r).map(|r| view(store, &scope, &r, false)))
        .collect::<AppResult<_>>()?;
    Ok(
        json!({"workspace":scope.name,"executions":entries,"next_offset":if more{Some(offset+limit)}else{None}}),
    )
}
pub fn recent(store: &Store) -> AppResult<Vec<Value>> {
    let scopes = store.scopes()?;
    records(store, None, None, 0, 50)?
        .into_iter()
        .filter_map(|r| {
            let scope = scopes.iter().find(|s| s.id == r.scope_id)?;
            Some(reconcile(store, r).map(|r| view(store, scope, &r, false)))
        })
        .collect()
}
pub fn get(store: &Store, p: RunArgs) -> AppResult<Value> {
    let (scope, record) = authorized(store, &p.workspace, &p.run_id)?;
    Ok(view(store, &scope, &record, true))
}
const WAIT_DEFAULT_SECONDS: u64 = 30;
const WAIT_MAX_SECONDS: u64 = 50;
fn wait_limit(timeout_seconds: Option<u64>) -> u64 {
    timeout_seconds
        .unwrap_or(WAIT_DEFAULT_SECONDS)
        .min(WAIT_MAX_SECONDS)
}
/// Waits until the execution leaves starting/running or the limit passes.
/// Each status check runs on a blocking thread; no new check starts once the budget is spent.
pub async fn wait(store: Arc<Store>, p: WaitArgs) -> AppResult<Value> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(wait_limit(p.timeout_seconds));
    loop {
        let (checked, workspace, run_id) = (store.clone(), p.workspace.clone(), p.run_id.clone());
        let (scope, record) =
            tokio::task::spawn_blocking(move || authorized(&checked, &workspace, &run_id))
                .await
                .map_err(err)??;
        let stopped = !matches!(record.status, Status::Starting | Status::Running);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !stopped && !remaining.is_zero() {
            tokio::time::sleep(remaining.min(Duration::from_millis(500))).await;
            if Instant::now() < deadline {
                continue;
            }
        }
        // Reports the latest snapshot; unknown also stops the wait but is not a confirmed end.
        let mut result = view(&store, &scope, &record, true);
        result["finished"] = json!(!record.status.active());
        result["waited_seconds"] =
            json!((started.elapsed().as_millis() as f64 / 100.0).round() / 10.0);
        return Ok(result);
    }
}
pub async fn command(store: Arc<Store>, p: CommandArgs) -> AppResult<Value> {
    if !cfg!(target_os = "macos") {
        return Err("直接命令执行当前仅支持 macOS 的系统沙箱".into());
    }
    if p.command.trim().is_empty() || p.command.len() > 16000 {
        return Err("命令不能为空且不能超过 16 KB".into());
    }
    launch(
        store,
        &p.workspace,
        p.request_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        Specification {
            kind: Kind::Shell,
            command: p.command,
            prompt: String::new(),
            acceptance_criteria: String::new(),
            session_id: None,
            timeout_seconds: p.timeout_seconds,
            delegation: None,
        },
        p.wait_seconds.unwrap_or(1),
    )
    .await
}
pub async fn codex(store: Arc<Store>, p: CodexArgs) -> AppResult<Value> {
    if !store.config().codex_enabled {
        return Err("本机已关闭 Codex 委派；请直接使用 fs_* 与 command_exec 完成".into());
    }
    if p.prompt.trim().is_empty() || p.prompt.len() > 65536 || p.acceptance_criteria.len() > 16000 {
        return Err("Codex 要求不能为空，最多 64 KiB；验收条件最多 16 KiB".into());
    }
    if p.session_id
        .as_ref()
        .is_some_and(|s| Uuid::parse_str(s).is_err())
    {
        return Err("请提供明确的 Codex 会话 UUID".into());
    }
    launch(
        store,
        &p.workspace,
        p.request_id,
        Specification {
            kind: Kind::Codex,
            command: String::new(),
            prompt: p.prompt,
            acceptance_criteria: p.acceptance_criteria,
            session_id: p.session_id,
            timeout_seconds: p.timeout_seconds,
            delegation: p.delegation,
        },
        p.wait_seconds.unwrap_or(1),
    )
    .await
}
async fn launch(
    store: Arc<Store>,
    workspace: &str,
    request_id: String,
    spec: Specification,
    wait: u64,
) -> AppResult<Value> {
    if request_id.trim().is_empty()
        || request_id.len() > 160
        || request_id.chars().any(char::is_control)
    {
        return Err("request_id 需为 1–160 字节，请在重试同一请求时复用".into());
    }
    let scope = store.scope(workspace)?;
    files::root(&store, &scope.id)?;
    if std::path::Path::new(&scope.path)
        .canonicalize()
        .map_err(err)?
        != std::path::Path::new(&scope.path)
    {
        return Err("授权目录路径已改变，请在本机重新授权".into());
    }
    let config = store.config();
    let timeout = spec
        .timeout_seconds
        .unwrap_or(config.command_timeout_seconds);
    if !(1..=86400).contains(&timeout) {
        return Err("执行时限应为 1–86400 秒".into());
    }
    let timestamp = now();
    let mut record = Record {
        run_id: Uuid::new_v4().to_string(),
        scope_id: scope.id.clone(),
        workspace: scope.name.clone(),
        request_id,
        session_id: spec.session_id.clone(),
        delegation: spec.delegation,
        spec,
        status: Status::Starting,
        codex_home: None,
        codex_path: config.codex_path,
        timeout_seconds: timeout,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
        heartbeat_at: timestamp,
        ended_at: None,
        worker_pid: None,
        worker_identity: None,
        child_pid: None,
        child_identity: None,
        cancel_reason: None,
        exit_code: None,
        signal: None,
        error: None,
        turn_completed: false,
        turn_failed: false,
        summary: String::new(),
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        stdout_bytes: 0,
        stderr_bytes: 0,
        verification: "unverified".into(),
        verification_note: String::new(),
        evidence_paths: Vec::new(),
    };
    let (record, is_new) = {
        let mut db = store.db.lock().unwrap();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM execution_records WHERE scope_id=? AND request_id=?",
                params![scope.id, record.request_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(raw) = existing {
            let existing: Record = serde_json::from_str(&raw).map_err(err)?;
            if existing.spec != record.spec {
                return Err(
                    "此 request_id 已用于不同内容，请复用原内容或使用新的 request_id".into(),
                );
            }
            (existing, false)
        } else {
            if record.spec.kind == Kind::Codex {
                record.codex_home = if let Some(session) = &record.session_id {
                    let raw: Option<String> = tx.query_row("SELECT payload FROM execution_records WHERE scope_id=? AND session_id=? AND kind='codex' ORDER BY created_at DESC LIMIT 1",
                        params![scope.id,session],|r|r.get(0)).optional().map_err(err)?;
                    let previous: Record =
                        serde_json::from_str(&raw.ok_or(
                            "该会话不属于此目录的已登记 Codex 执行，请先查询 execution_list",
                        )?)
                        .map_err(err)?;
                    record.delegation = record.delegation.or(previous.delegation);
                    previous.codex_home
                } else {
                    let home = std::env::var_os("CODEX_HOME")
                        .map(std::path::PathBuf::from)
                        .or_else(|| dirs::home_dir().map(|p| p.join(".codex")))
                        .ok_or("无法确定 Codex 会话存储目录")?;
                    let home = if home.is_absolute() {
                        home
                    } else {
                        std::env::current_dir().map_err(err)?.join(home)
                    };
                    Some(home.to_string_lossy().into_owned())
                };
            }
            tx.execute("INSERT INTO execution_records (run_id,scope_id,request_id,kind,status,session_id,created_at,payload) VALUES (?,?,?,?,?,?,?,?)",params![
                record.run_id,record.scope_id,record.request_id,record.spec.kind.key(),record.status.key(),
                record.session_id,record.created_at,serde_json::to_string(&record).map_err(err)?,
            ]).map_err(|e|if matches!(e,rusqlite::Error::SqliteFailure(ref code,_) if code.code==rusqlite::ErrorCode::ConstraintViolation){
                "此目录或 Codex 会话已有执行正在运行或等待核实，请查询 execution_list 后接续".into()
            }else{err(e)})?;
            tx.commit().map_err(err)?;
            (record, true)
        }
    };
    if is_new {
        if let Err(error) = spawn_worker(&store, &record.run_id) {
            change(&store, &record.run_id, |r| {
                r.status = Status::Failed;
                r.error = Some(format!("执行器启动失败：{error}"));
                r.ended_at = Some(now());
                Ok(())
            })?;
        }
        store.audit("execution.start", &scope.name, true);
    }
    let deadline = Instant::now() + Duration::from_secs(wait.min(10));
    loop {
        let current = reconcile(&store, load(&store, &record.run_id)?)?;
        if !current.status.active() || Instant::now() >= deadline {
            let mut result = view(&store, &scope, &current, true);
            result["reused_request"] = json!(!is_new);
            return Ok(result);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn spawn_worker(store: &Store, id: &str) -> AppResult<()> {
    let dir = store.dir.join("executions").join(id);
    fs::create_dir_all(&dir).map_err(err)?;
    let mut cmd = std::process::Command::new(std::env::current_exe().map_err(err)?);
    cmd.arg("--execution-worker")
        .arg(&store.dir)
        .arg(id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(dir.join("worker.log")).map_err(err)?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    let mut child = cmd.spawn().map_err(err)?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn output(store: &Store, p: OutputArgs) -> AppResult<Value> {
    let (_, r) = authorized(store, &p.workspace, &p.run_id)?;
    let offset = p.offset.unwrap_or(0);
    let limit = p.max_bytes.unwrap_or(12000).clamp(1, 32768);
    let text = match p.stream.as_str() {
        "prompt" => Some(r.spec.prompt.as_str()),
        "acceptance_criteria" => Some(r.spec.acceptance_criteria.as_str()),
        "summary" => Some(r.summary.as_str()),
        _ => None,
    };
    let (bytes, total) = if let Some(text) = text {
        let raw = text.as_bytes();
        let start = (offset.min(raw.len() as u64)) as usize;
        (
            raw[start..raw.len().min(start + limit)].to_vec(),
            raw.len() as u64,
        )
    } else {
        let file =
            match p.stream.as_str() {
                "stdout" => "stdout.log",
                "stderr" => "stderr.log",
                "worker" => "worker.log",
                _ => return Err(
                    "stream 应为 stdout、stderr、prompt、acceptance_criteria、summary 或 worker"
                        .into(),
                ),
            };
        let path = store.dir.join("executions").join(&r.run_id).join(file);
        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(
                    json!({"text":"","offset":offset,"next_offset":offset,"total_bytes":0,"pending":r.status.active()}),
                )
            }
            Err(e) => return Err(err(e)),
        };
        let total = file.metadata().map_err(err)?.len();
        file.seek(SeekFrom::Start(offset)).map_err(err)?;
        let mut bytes = Vec::new();
        file.take(limit as u64)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        (bytes, total)
    };
    let next = offset.saturating_add(bytes.len() as u64);
    Ok(
        json!({"run_id":r.run_id,"stream":p.stream,"text":privacy::text(&String::from_utf8_lossy(&bytes)),"offset":offset,"next_offset":next,"total_bytes":total,"more":next<total,"running":r.status.active(),"log_limit_bytes":LOG_LIMIT}),
    )
}
pub fn cancel(store: &Store, p: RunArgs) -> AppResult<Value> {
    let (scope, _) = authorized(store, &p.workspace, &p.run_id)?;
    let record = change(store, &p.run_id, |r| {
        if r.status.active() {
            r.cancel_reason = Some("Agent 或本机用户请求停止".into());
        }
        Ok(())
    })?;
    stop_orphan(&record);
    Ok(view(store, &scope, &record, true))
}
fn stop_orphan(record: &Record) {
    if record.status == Status::Unknown
        && record.child_identity.is_some()
        && worker::process_matches(record.child_pid, record.child_identity.as_deref())
    {
        crate::shell::terminate(record.child_pid);
        worker::force_group(record.child_pid);
    }
}
pub fn cancel_scope(store: &Store, scope: Option<&str>, reason: &str) -> AppResult<()> {
    let ids: Vec<String> = {
        let db = store.db.lock().unwrap();
        let mut q = db.prepare("SELECT run_id FROM execution_records WHERE (?1 IS NULL OR scope_id=?1) AND status IN ('starting','running','unknown')").map_err(err)?;
        let rows = q.query_map([scope], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for id in ids {
        let record = change(store, &id, |r| {
            if r.status.active() {
                r.cancel_reason = Some(reason.into());
            }
            Ok(())
        })?;
        stop_orphan(&record);
    }
    Ok(())
}
pub fn review(store: &Store, p: ReviewArgs) -> AppResult<Value> {
    let (scope, record) = authorized(store, &p.workspace, &p.run_id)?;
    if !matches!(
        p.verification.as_str(),
        "unverified" | "completed" | "needs_work"
    ) || p.note.trim().is_empty()
        || p.note.len() > 16000
        || p.evidence_paths.len() > 30
    {
        return Err("请指定 unverified / completed / needs_work，并提供不超过 16 KiB 的检查说明；证据路径最多 30 项".into());
    }
    for path in &p.evidence_paths {
        if path.len() > 1024 {
            return Err("单个证据相对路径不能超过 1024 字节".into());
        }
        files::relative(path, false)?;
    }
    if record.status == Status::Unknown
        && p.confirm_stopped
        && (worker::process_matches(record.worker_pid, record.worker_identity.as_deref())
            || worker::process_matches(record.child_pid, record.child_identity.as_deref()))
    {
        return Err("已登记进程仍在运行，请先停止并确认后再解除状态待核实".into());
    }
    let record = change(store, &p.run_id, |r| {
        if r.status == Status::Unknown && p.confirm_stopped {
            r.status = Status::Interrupted;
            r.ended_at = Some(now());
            r.error = Some("接手者核实执行已停止；原终止原因未知，验收结论见检查说明".into());
        }
        if r.status.active() {
            return Err("执行仍在运行或尚未确认停止，不能标记验收状态".into());
        }
        r.verification = p.verification;
        r.verification_note = p.note;
        r.evidence_paths = p.evidence_paths;
        Ok(())
    })?;
    Ok(view(store, &scope, &record, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_from_the_task_version_still_load_and_insert() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("project");
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        store.add_scope(&root, Some("project")).unwrap();
        store
            .db
            .lock()
            .unwrap()
            .execute_batch("ALTER TABLE execution_records ADD COLUMN task_id TEXT")
            .unwrap();
        let record = Record {
            run_id: Uuid::new_v4().to_string(),
            scope_id: store.scope("project").unwrap().id,
            workspace: "project".into(),
            request_id: "r1".into(),
            spec: Specification {
                kind: Kind::Shell,
                command: "true".into(),
                prompt: String::new(),
                acceptance_criteria: String::new(),
                session_id: None,
                timeout_seconds: None,
                delegation: None,
            },
            delegation: None,
            status: Status::Succeeded,
            session_id: None,
            codex_home: None,
            codex_path: "codex".into(),
            timeout_seconds: 60,
            created_at: now(),
            updated_at: now(),
            heartbeat_at: now(),
            ended_at: Some(now()),
            worker_pid: None,
            worker_identity: None,
            child_pid: None,
            child_identity: None,
            cancel_reason: None,
            exit_code: Some(0),
            signal: None,
            error: None,
            turn_completed: false,
            turn_failed: false,
            summary: String::new(),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 0,
            verification: "unverified".into(),
            verification_note: String::new(),
            evidence_paths: Vec::new(),
        };
        let mut payload = serde_json::to_value(&record).unwrap();
        for field in ["verification", "verification_note", "evidence_paths"] {
            payload.as_object_mut().unwrap().remove(field);
        }
        payload["spec"]["task_id"] = json!("t1");
        store.db.lock().unwrap().execute(
            "INSERT INTO execution_records (run_id,scope_id,request_id,kind,status,session_id,created_at,payload,task_id) VALUES (?,?,?,?,?,?,?,?,?)",
            params![record.run_id, record.scope_id, record.request_id, "shell", "succeeded", None::<String>, record.created_at, payload.to_string(), "t1"],
        ).unwrap();
        let loaded = load(&store, &record.run_id).unwrap();
        assert_eq!(loaded.verification, "unverified");
        let listed = list(&store, ListArgs { workspace: "project".into(), kind: None, offset: None, limit: None }).unwrap();
        assert_eq!(listed["executions"].as_array().unwrap().len(), 1);
    }
    fn insert_record(store: &Store, status: Status) -> String {
        let timestamp = now();
        let record = Record {
            run_id: Uuid::new_v4().to_string(),
            scope_id: store.scope("project").unwrap().id,
            workspace: "project".into(),
            request_id: Uuid::new_v4().to_string(),
            spec: Specification {
                kind: Kind::Shell,
                command: "true".into(),
                prompt: String::new(),
                acceptance_criteria: String::new(),
                session_id: None,
                timeout_seconds: None,
                delegation: None,
            },
            delegation: None,
            status,
            session_id: None,
            codex_home: None,
            codex_path: "codex".into(),
            timeout_seconds: 60,
            created_at: timestamp.clone(),
            updated_at: timestamp.clone(),
            heartbeat_at: timestamp,
            ended_at: None,
            worker_pid: None,
            worker_identity: None,
            child_pid: None,
            child_identity: None,
            cancel_reason: None,
            exit_code: None,
            signal: None,
            error: None,
            turn_completed: false,
            turn_failed: false,
            summary: String::new(),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 0,
            verification: "unverified".into(),
            verification_note: String::new(),
            evidence_paths: Vec::new(),
        };
        store.db.lock().unwrap().execute(
            "INSERT INTO execution_records (run_id,scope_id,request_id,kind,status,session_id,created_at,payload) VALUES (?,?,?,?,?,?,?,?)",
            params![record.run_id, record.scope_id, record.request_id, "shell", status.key(), None::<String>, record.created_at, serde_json::to_string(&record).unwrap()],
        ).unwrap();
        record.run_id
    }

    fn wait_store() -> (tempfile::TempDir, Arc<Store>) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("project");
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        store.add_scope(&root, Some("project")).unwrap();
        (temp, store)
    }

    #[tokio::test]
    async fn wait_returns_at_once_for_a_finished_execution() {
        let (_temp, store) = wait_store();
        let run_id = insert_record(&store, Status::Succeeded);
        let started = Instant::now();
        let result = wait(
            store.clone(),
            WaitArgs {
                workspace: "project".into(),
                run_id: run_id.clone(),
                timeout_seconds: Some(50),
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result["finished"], json!(true));
        assert_eq!(result["run_id"], json!(run_id));
        assert_eq!(result["status"], json!("succeeded"));
    }

    #[tokio::test]
    async fn wait_times_out_while_the_execution_runs() {
        let (_temp, store) = wait_store();
        let run_id = insert_record(&store, Status::Running);
        let result = wait(
            store.clone(),
            WaitArgs {
                workspace: "project".into(),
                run_id,
                timeout_seconds: Some(1),
            },
        )
        .await
        .unwrap();
        assert_eq!(result["finished"], json!(false));
        let waited = result["waited_seconds"].as_f64().unwrap();
        assert!((1.0..3.0).contains(&waited), "{waited}");
    }

    #[tokio::test]
    async fn wait_rejects_missing_runs() {
        let (_temp, store) = wait_store();
        let missing = WaitArgs {
            workspace: "project".into(),
            run_id: Uuid::new_v4().to_string(),
            timeout_seconds: Some(0),
        };
        assert!(wait(store, missing).await.is_err());
    }

    #[tokio::test]
    async fn wait_returns_when_a_running_execution_ends() {
        let (_temp, store) = wait_store();
        let run_id = insert_record(&store, Status::Running);
        let check = WaitArgs {
            workspace: "project".into(),
            run_id: run_id.clone(),
            timeout_seconds: Some(0),
        };
        let first = wait(store.clone(), check).await.unwrap();
        assert_eq!(first["finished"], json!(false));
        let (finisher, id) = (store.clone(), run_id.clone());
        tokio::spawn(async move {
            // Long enough that the waiter's first check sees the running state.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            change(&finisher, &id, |r| {
                r.status = Status::Succeeded;
                Ok(())
            })
            .unwrap();
        });
        let started = Instant::now();
        let result = wait(
            store.clone(),
            WaitArgs {
                workspace: "project".into(),
                run_id,
                timeout_seconds: Some(20),
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(result["waited_seconds"].as_f64().unwrap() >= 1.0);
        assert_eq!(result["finished"], json!(true));
        assert_eq!(result["status"], json!("succeeded"));
    }

    #[tokio::test]
    async fn wait_stops_on_unknown_without_reporting_finished() {
        let (_temp, store) = wait_store();
        let run_id = insert_record(&store, Status::Unknown);
        let started = Instant::now();
        let result = wait(
            store.clone(),
            WaitArgs {
                workspace: "project".into(),
                run_id,
                timeout_seconds: Some(20),
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result["finished"], json!(false));
        assert_eq!(result["status"], json!("unknown"));
    }

    #[test]
    fn wait_limit_defaults_and_clamps() {
        assert_eq!(wait_limit(None), 30);
        assert_eq!(wait_limit(Some(0)), 0);
        assert_eq!(wait_limit(Some(500)), 50);
    }
}
