use crate::{
    shell::terminate,
    store::{err, find_binary, AppResult, Store},
};
#[cfg(target_os = "macos")]
use std::path::Path;
use std::{path::PathBuf, process::Stdio, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub fn executable(configured: &str) -> PathBuf {
    if configured != "cloudflared" {
        return configured.into();
    }
    #[cfg(target_os = "macos")]
    if let Some(path) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(bundled_executable)
    {
        return path;
    }
    find_binary("cloudflared").into()
}

#[cfg(target_os = "macos")]
fn bundled_executable(app_executable: &Path) -> Option<PathBuf> {
    let directory = app_executable.parent()?;
    let app = directory.parent()?.parent()?;
    if !directory.ends_with("Contents/MacOS") || app.extension()? != "app" {
        return None;
    }
    let sidecar = directory.join("cloudflared");
    sidecar.is_file().then_some(sidecar)
}

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
    let mut cmd = Command::new(executable(&config.cloudflared_path));
    cmd.args([
        "tunnel",
        "--no-autoupdate",
        "--http-host-header",
        &format!("127.0.0.1:{}", config.port),
    ]);
    if config.tunnel_mode == "named" {
        match crate::cloudflare::managed_tunnel_id(&store, &config.tunnel_name) {
            // A tunnel CloudMCP created after a Cloudflare login: its credentials live in the
            // app data directory and the origin is passed directly, so no config.yml is needed.
            Some(id) => {
                let creds = crate::cloudflare::credentials_path(&store, &config.tunnel_name);
                cmd.env("HOME", crate::cloudflare::home(&store)).args([
                    "run",
                    "--credentials-file",
                    &creds.to_string_lossy(),
                    "--url",
                    &format!("http://127.0.0.1:{}", config.port),
                    &id,
                ]);
            }
            None => {
                cmd.args(["run", &config.tunnel_name]);
            }
        }
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
        .map_err(|e| format!("无法启动 cloudflared：{e}。请在设置中填写 cloudflared 自动选择内置版，或填写本机可执行文件的路径。"))?;
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::bundled_executable;
    use std::fs;

    #[test]
    fn finds_cloudflared_only_inside_the_application_bundle() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("cloudflared"), []).unwrap();
        assert_eq!(bundled_executable(&dir.path().join("cloudmcp")), None);

        let macos = dir.path().join("CloudMCP.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        let app = macos.join("cloudmcp");
        assert_eq!(bundled_executable(&app), None);

        let sidecar = macos.join("cloudflared");
        fs::write(&sidecar, []).unwrap();
        assert_eq!(bundled_executable(&app), Some(sidecar));
    }
}
