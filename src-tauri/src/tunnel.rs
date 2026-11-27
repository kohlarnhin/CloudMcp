use crate::{
    shell::terminate,
    store::{find_binary, AppResult, Settings, Store},
};
#[cfg(target_os = "macos")]
use std::path::Path;
use std::{
    collections::HashSet,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
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

/// How often the supervisor checks the tunnel.
const TICK: Duration = Duration::from_secs(15);
/// A wall-clock gap this much longer than a tick means the Mac was asleep.
const SLEEP_GAP: Duration = Duration::from_secs(45);
/// How long the tunnel may stay without any edge connection before cloudflared is restarted.
const DOWN_LIMIT: Duration = Duration::from_secs(60);
/// How often the public address is probed while the tunnel looks connected.
const PROBE_EVERY: Duration = Duration::from_secs(60);
/// Consecutive failed probes that force a restart.
const PROBE_FAILURES: u32 = 2;
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const RECONNECTING: &str = "隧道连接中断，正在自动重连…";

/// What the cloudflared log says about the tunnel.
#[derive(Default)]
struct Health {
    /// Edge connections currently registered, by connIndex.
    connections: HashSet<u32>,
    /// The quick tunnel address, once cloudflared prints it.
    quick_url: Option<String>,
    /// Whether this tunnel has been online at least once.
    seen_online: bool,
}

#[derive(Debug, PartialEq)]
enum LogEvent {
    Up(u32),
    Down(u32),
    QuickUrl(String),
}

fn conn_index(line: &str) -> u32 {
    line.split("connIndex=")
        .nth(1)
        .and_then(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

fn parse_line(line: &str) -> Option<LogEvent> {
    if let Some(start) = line.find("https://") {
        let url = line[start..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(['|', ',']);
        if url.ends_with(".trycloudflare.com") {
            return Some(LogEvent::QuickUrl(url.into()));
        }
    }
    if line.contains("Unregistered tunnel connection")
        || line.contains("Connection terminated")
        || line.contains("Serve tunnel error")
        || line.contains("Retrying connection")
        || line.contains("Lost connection")
    {
        return Some(LogEvent::Down(conn_index(line)));
    }
    if line.contains("Registered tunnel connection") {
        return Some(LogEvent::Up(conn_index(line)));
    }
    None
}

fn base_url(config: &Settings, health: &Health) -> Option<String> {
    if config.tunnel_mode == "named" {
        Some(
            config
                .public_url
                .trim_end_matches('/')
                .trim_end_matches("/mcp")
                .into(),
        )
    } else {
        health.quick_url.clone()
    }
}

/// Mirrors the tunnel's health into the snapshot the window shows, but only for the current run.
fn publish(
    store: &Store,
    generation: &str,
    config: &Settings,
    health: &mut Health,
    note: Option<String>,
) {
    let url = (!health.connections.is_empty())
        .then(|| base_url(config, health))
        .flatten();
    if url.is_some() {
        health.seen_online = true;
    }
    let mut live = store.live.lock().unwrap();
    if live.tunnel_id.as_deref() != Some(generation) {
        return;
    }
    live.tunnel_error = match (&url, note) {
        (Some(_), _) => None,
        (None, Some(note)) => Some(note),
        (None, None) if health.seen_online => Some(RECONNECTING.into()),
        (None, None) => None,
    };
    live.tunnel_url = url;
}

fn command(store: &Store, config: &Settings) -> Command {
    let mut cmd = Command::new(executable(&config.cloudflared_path));
    cmd.args([
        "tunnel",
        "--no-autoupdate",
        "--http-host-header",
        &format!("127.0.0.1:{}", config.port),
    ]);
    if config.tunnel_mode == "named" {
        match crate::cloudflare::managed_tunnel_id(store, &config.tunnel_name) {
            // A tunnel CloudMCP created after a Cloudflare login: its credentials live in the
            // app data directory and the origin is passed directly, so no config.yml is needed.
            Some(id) => {
                let creds = crate::cloudflare::credentials_path(store, &config.tunnel_name);
                cmd.env("HOME", crate::cloudflare::home(store)).args([
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
    cmd
}

fn spawn(store: &Store, config: &Settings) -> AppResult<Child> {
    command(store, config)
        .spawn()
        .map_err(|e| format!("无法启动 cloudflared：{e}。请在设置中填写 cloudflared 自动选择内置版，或填写本机可执行文件的路径。"))
}

/// Whether the public address still reaches CloudMCP. Any answer from CloudMCP itself (401, 405,
/// 406 …) counts; Cloudflare's 530 (no tunnel connector) or no answer at all does not.
async fn probe(url: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return true;
    };
    match client.get(format!("{url}/mcp")).send().await {
        Ok(response) => !matches!(response.status().as_u16(), 530),
        // Only a connection failure or timeout means the address is down; anything else is a
        // problem with this probe, not with the tunnel.
        Err(e) => !(e.is_timeout() || e.is_connect()),
    }
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
    let child = spawn(&store, &config)?;
    let cancel = CancellationToken::new();
    let generation = uuid::Uuid::new_v4().to_string();
    {
        let mut live = store.live.lock().unwrap();
        live.tunnel = Some(cancel.clone());
        live.tunnel_error = None;
        live.tunnel_url = None;
        live.tunnel_id = Some(generation.clone());
    }
    store.audit("tunnel.start", "", true);
    tauri::async_runtime::spawn(supervise(store, config, cancel, generation, child));
    Ok(())
}

/// Keeps cloudflared running: follows its connections, probes the public address, notices the
/// Mac waking from sleep, and restarts cloudflared with backoff when the tunnel stays down or the
/// process exits.
async fn supervise(
    store: Arc<Store>,
    config: Settings,
    cancel: CancellationToken,
    generation: String,
    first: Child,
) {
    let health = Arc::new(Mutex::new(Health::default()));
    let mut next = Some(first);
    let mut backoff = Duration::from_secs(2);
    loop {
        let mut child = match next.take() {
            Some(child) => child,
            None => match spawn(&store, &config) {
                Ok(child) => child,
                Err(e) => {
                    publish(
                        &store,
                        &generation,
                        &config,
                        &mut health.lock().unwrap(),
                        Some(e),
                    );
                    if !pause(&cancel, &mut backoff).await {
                        return;
                    }
                    continue;
                }
            },
        };
        let pid = child.id();
        {
            let mut h = health.lock().unwrap();
            h.connections.clear();
            h.quick_url = None;
        }
        let reader = {
            let stderr = child.stderr.take().unwrap();
            let (store, config, generation, health) = (
                store.clone(),
                config.clone(),
                generation.clone(),
                health.clone(),
            );
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Some(event) = parse_line(&line) else {
                        continue;
                    };
                    let mut h = health.lock().unwrap();
                    match event {
                        LogEvent::Up(i) => {
                            h.connections.insert(i);
                        }
                        LogEvent::Down(i) => {
                            h.connections.remove(&i);
                        }
                        LogEvent::QuickUrl(url) => h.quick_url = Some(url),
                    }
                    publish(&store, &generation, &config, &mut h, None);
                }
            })
        };

        let started = Instant::now();
        let mut wall = SystemTime::now();
        let mut down_since = Some(Instant::now());
        let mut last_probe = Instant::now();
        let mut failed_probes = 0;
        let reason = loop {
            tokio::select! {
                status = child.wait() => {
                    break format!(
                        "cloudflared 已退出（{}）",
                        status.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string())
                    );
                }
                _ = cancel.cancelled() => {
                    reader.abort();
                    terminate(pid);
                    let _ = child.kill().await;
                    return;
                }
                _ = tokio::time::sleep(TICK) => {}
            }
            let now = SystemTime::now();
            let woke = now
                .duration_since(wall)
                .is_ok_and(|gap| gap > TICK + SLEEP_GAP);
            wall = now;
            let (connected, url) = {
                let h = health.lock().unwrap();
                (!h.connections.is_empty(), base_url(&config, &h))
            };
            if woke {
                // Connections cloudflared still believes in may be dead after sleep: check now.
                store.audit("tunnel.wake", "", true);
                last_probe = Instant::now() - PROBE_EVERY;
            }
            if connected {
                down_since = None;
            } else {
                let since = *down_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= DOWN_LIMIT {
                    break "隧道超过 60 秒没有连上 Cloudflare".into();
                }
                continue;
            }
            if let Some(url) = url {
                if last_probe.elapsed() >= PROBE_EVERY {
                    last_probe = Instant::now();
                    if probe(&url).await {
                        failed_probes = 0;
                    } else {
                        failed_probes += 1;
                        if failed_probes >= PROBE_FAILURES {
                            break "公网地址无法访问 CloudMCP".into();
                        }
                        // Probe again on the next tick.
                        last_probe = Instant::now() - PROBE_EVERY;
                    }
                }
            }
        };
        reader.abort();
        terminate(pid);
        let _ = child.kill().await;
        store.audit("tunnel.restart", &reason, false);
        if started.elapsed() > Duration::from_secs(300) {
            backoff = Duration::from_secs(2);
        }
        {
            let mut h = health.lock().unwrap();
            h.connections.clear();
            let note = format!("{reason}，{} 秒后自动重连…", backoff.as_secs());
            h.seen_online = true;
            publish(&store, &generation, &config, &mut h, Some(note));
        }
        if !pause(&cancel, &mut backoff).await {
            return;
        }
    }
}

/// Waits out the backoff; false when the tunnel was stopped meanwhile.
async fn pause(cancel: &CancellationToken, backoff: &mut Duration) -> bool {
    let wait = *backoff;
    *backoff = (*backoff * 2).min(MAX_BACKOFF);
    tokio::select! {
        _ = tokio::time::sleep(wait) => true,
        _ = cancel.cancelled() => false,
    }
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

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn reads_connection_events_from_cloudflared_logs() {
        assert_eq!(
            parse_line("2026-11-27T11:00:00Z INF Registered tunnel connection connIndex=2 connection=abc event=0 ip=198.41.200.13 location=nrt01 protocol=quic"),
            Some(LogEvent::Up(2))
        );
        assert_eq!(
            parse_line("2026-11-27T11:05:00Z ERR Serve tunnel error error=\"timeout: no recent network activity\" connIndex=2 event=0"),
            Some(LogEvent::Down(2))
        );
        assert_eq!(
            parse_line("INF Unregistered tunnel connection connIndex=0 event=0"),
            Some(LogEvent::Down(0))
        );
        assert_eq!(
            parse_line("INF |  https://brave-fox-river.trycloudflare.com  |"),
            Some(LogEvent::QuickUrl(
                "https://brave-fox-river.trycloudflare.com".into()
            ))
        );
        assert_eq!(
            parse_line("INF Starting metrics server on 127.0.0.1:20241/metrics"),
            None
        );
    }

    #[test]
    fn the_address_disappears_while_no_connection_is_up() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        let config = Settings {
            tunnel_mode: "named".into(),
            public_url: "https://mcp.example.com/mcp".into(),
            ..store.config()
        };
        store.live.lock().unwrap().tunnel_id = Some("g".into());
        let mut health = Health::default();
        publish(&store, "g", &config, &mut health, None);
        assert_eq!(store.live.lock().unwrap().tunnel_url, None);
        assert_eq!(store.live.lock().unwrap().tunnel_error, None);

        health.connections.insert(0);
        publish(&store, "g", &config, &mut health, None);
        assert_eq!(
            store.live.lock().unwrap().tunnel_url.as_deref(),
            Some("https://mcp.example.com")
        );

        health.connections.clear();
        publish(&store, "g", &config, &mut health, None);
        let live = store.live.lock().unwrap();
        assert_eq!(live.tunnel_url, None);
        assert_eq!(live.tunnel_error.as_deref(), Some(RECONNECTING));
    }
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
