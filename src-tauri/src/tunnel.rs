use crate::{
    shell::terminate,
    store::{err, AppResult, Store},
};
use std::{process::Stdio, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub async fn start(store: Arc<Store>) -> AppResult<()> {
    let config = store.config();
    {
        let live = store.live.lock().unwrap();
        if live.server.is_none() {
            return Err("请先启动本机 MCP 服务".into());
        }
        if live.tunnel.is_some() {
            return Ok(());
        }
    }
    if config.tunnel_mode == "named"
        && (config.tunnel_name.trim().is_empty() || !config.public_url.starts_with("https://"))
    {
        return Err("请填写已配置的隧道名称和 HTTPS 公网地址".into());
    }
    let mut cmd = Command::new(&config.cloudflared_path);
    cmd.args([
        "tunnel",
        "--no-autoupdate",
        "--http-host-header",
        &format!("127.0.0.1:{}", config.port),
    ]);
    if config.tunnel_mode == "named" {
        cmd.args(["run", &config.tunnel_name]);
    } else {
        cmd.args(["--url", &format!("http://127.0.0.1:{}", config.port)]);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 cloudflared：{e}。请安装后在设置中填写可执行文件路径。"))?;
    let pid = child.id();
    let cancel = CancellationToken::new();
    let generation = uuid::Uuid::new_v4().to_string();
    {
        let mut live = store.live.lock().unwrap();
        live.tunnel = Some(cancel.clone());
        live.tunnel_error = None;
        live.tunnel_url = None;
        live.tunnel_id = Some(generation.clone());
    }
    let stderr = child.stderr.take().unwrap();
    let state = store.clone();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(start) = line.find("https://") {
                let url = line[start..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(['|', ',']);
                if url.ends_with(".trycloudflare.com") {
                    state.live.lock().unwrap().tunnel_url = Some(url.into());
                }
            }
            if line.contains("Registered tunnel connection") && config.tunnel_mode == "named" {
                state.live.lock().unwrap().tunnel_url = Some(
                    config
                        .public_url
                        .trim_end_matches('/')
                        .trim_end_matches("/mcp")
                        .into(),
                );
            }
        }
    });
    store.audit("tunnel.start", "", true);
    tauri::async_runtime::spawn(async move {
        let result = tokio::select! {r=child.wait()=>r.map_err(err),_=cancel.cancelled()=>{terminate(pid);let _=child.kill().await;return;}};
        reader.abort();
        let mut live = store.live.lock().unwrap();
        if live.tunnel_id.as_ref() != Some(&generation) {
            return;
        }
        live.tunnel = None;
        live.tunnel_url = None;
        live.tunnel_error = Some(format!(
            "隧道已退出：{}",
            result.map(|s| s.to_string()).unwrap_or_else(|e| e)
        ));
    });
    Ok(())
}
pub fn stop(store: &Store) {
    let mut live = store.live.lock().unwrap();
    if let Some(c) = live.tunnel.take() {
        c.cancel();
    }
    live.tunnel_url = None;
    live.tunnel_error = None;
    live.tunnel_id = None;
}
