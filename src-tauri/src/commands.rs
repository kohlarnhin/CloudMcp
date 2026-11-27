use crate::{
    cloudflare, executions, portal, privacy, proxy, server,
    store::{AppResult, Settings, Store},
    tunnel,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tauri::State;

#[tauri::command]
pub fn snapshot(state: State<'_, Arc<Store>>) -> AppResult<Value> {
    let scopes = state.scopes()?;
    let audit = state.audits()?;
    let executions = executions::recent(&state)?;
    let settings = state.config();
    let live = state.live.lock().unwrap();
    // Configuration executable paths stay local; public workspace paths are normalized.
    Ok(
        json!({"settings":settings,"scopes":privacy::value(json!(scopes)),"executions":privacy::value(json!(executions)),"audit":privacy::value(json!(audit)),"server_running":live.server.is_some(),"tunnel_running":live.tunnel.is_some(),"public_url":live.tunnel_url,"tunnel_error":live.tunnel_error.as_ref().map(|e|privacy::text(e)),"data_dir":privacy::text(&state.dir.to_string_lossy()),"cloudflare":cloudflare::status(&state),"platform":std::env::consts::OS}),
    )
}
#[tauri::command]
pub fn add_scope(
    state: State<'_, Arc<Store>>,
    path: String,
    name: Option<String>,
) -> AppResult<Value> {
    state
        .add_scope(&PathBuf::from(path), name.as_deref())
        .map(|s| privacy::value(json!(s)))
        .map_err(|e| privacy::text(&e))
}
#[tauri::command]
pub fn remove_scope(state: State<'_, Arc<Store>>, id: String) -> AppResult<()> {
    state.remove_scope(&id)
}
#[tauri::command]
pub fn rename_scope(state: State<'_, Arc<Store>>, id: String, name: String) -> AppResult<()> {
    state.rename_scope(&id, &name)
}
#[tauri::command]
pub fn execution_detail(
    state: State<'_, Arc<Store>>,
    workspace: String,
    run_id: String,
) -> AppResult<Value> {
    executions::get(&state, executions::RunArgs { workspace, run_id })
        .map(privacy::value)
        .map_err(|e| privacy::text(&e))
}
#[tauri::command]
pub fn cancel_execution(
    state: State<'_, Arc<Store>>,
    workspace: String,
    run_id: String,
) -> AppResult<Value> {
    executions::cancel(&state, executions::RunArgs { workspace, run_id })
        .map(privacy::value)
        .map_err(|e| privacy::text(&e))
}
#[tauri::command]
pub fn save_settings(state: State<'_, Arc<Store>>, settings: Settings) -> AppResult<()> {
    state.save_settings(settings)
}
#[tauri::command]
pub async fn start_server(state: State<'_, Arc<Store>>) -> AppResult<()> {
    server::start(state.inner().clone()).await
}
#[tauri::command]
pub fn stop_server(state: State<'_, Arc<Store>>) {
    server::stop(&state)
}
#[tauri::command]
pub async fn start_tunnel(state: State<'_, Arc<Store>>) -> AppResult<()> {
    tunnel::start(state.inner().clone()).await
}
#[tauri::command]
pub fn stop_tunnel(state: State<'_, Arc<Store>>) {
    tunnel::stop(&state)
}
#[tauri::command]
pub fn get_token(state: State<'_, Arc<Store>>) -> String {
    state.token.lock().unwrap().clone()
}
#[tauri::command]
pub fn rotate_token(state: State<'_, Arc<Store>>) -> AppResult<String> {
    state.rotate_token()
}
#[tauri::command]
pub async fn dependencies(state: State<'_, Arc<Store>>) -> AppResult<Value> {
    let config = state.config();
    let mut entries = Vec::new();
    let mut checks = vec![("cloudflared", tunnel::executable(&config.cloudflared_path))];
    if config.codex_enabled {
        checks.push(("codex", PathBuf::from(&config.codex_path)));
    }
    if config.pi_enabled() {
        checks.push(("pi", PathBuf::from(&config.pi_path)));
    }
    for (name, path) in checks {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new(&path)
                .arg("--version")
                .output(),
        )
        .await;
        let (available, version) = match output {
            Ok(Ok(o)) => (
                o.status.success(),
                crate::store::clipped(String::from_utf8_lossy(&o.stdout).trim(), 160),
            ),
            _ => (false, "未找到或无法启动".into()),
        };
        entries.push(json!({"name":name,"path":path.to_string_lossy(),"available":available,"version":privacy::text(&version)}));
    }
    Ok(json!(entries))
}

/// 打开浏览器登录 Cloudflare，等待授权完成（最长 10 分钟）。
#[tauri::command]
pub async fn cloudflare_login(state: State<'_, Arc<Store>>) -> AppResult<()> {
    cloudflare::login(&state).await
}
#[tauri::command]
pub fn cloudflare_logout(state: State<'_, Arc<Store>>) -> AppResult<()> {
    cloudflare::logout(&state)
}
/// 为域名自动创建或复用固定隧道、配置 DNS，并切换到固定隧道设置。
#[tauri::command]
pub async fn cloudflare_setup(state: State<'_, Arc<Store>>, hostname: String) -> AppResult<Value> {
    cloudflare::setup(&state, &hostname)
        .await
        .map_err(|e| privacy::text(&e))
}

/// 转发的本机 MCP 服务器及其连接状态。
#[tauri::command]
pub fn proxy_list(state: State<'_, Arc<Store>>) -> AppResult<Value> {
    let servers = proxy::list(&state)?;
    let entries: Vec<_> = servers
        .into_iter()
        .map(|s| {
            let connected = proxy::connected(&state, &s.name);
            let mut v = json!(s);
            v["connected"] = json!(connected);
            v
        })
        .collect();
    Ok(json!(entries))
}
#[tauri::command]
pub fn proxy_save(
    state: State<'_, Arc<Store>>,
    server: proxy::ProxyServer,
    original: Option<String>,
) -> AppResult<()> {
    proxy::save(&state, server, original.as_deref())
}
#[tauri::command]
pub fn proxy_remove(state: State<'_, Arc<Store>>, name: String) -> AppResult<()> {
    proxy::remove(&state, &name)
}
#[tauri::command]
pub fn proxy_toggle(state: State<'_, Arc<Store>>, name: String, enabled: bool) -> AppResult<()> {
    proxy::set_enabled(&state, &name, enabled)
}
/// 连接服务器并列出工具名，用于本机检查配置。
#[tauri::command]
pub async fn proxy_test(state: State<'_, Arc<Store>>, name: String) -> AppResult<Value> {
    proxy::test(state.inner(), &name)
        .await
        .map_err(|e| privacy::text(&e))
}
#[tauri::command]
pub fn proxy_import_sources() -> Value {
    json!(proxy::import_sources())
}
#[tauri::command]
pub fn proxy_import(
    state: State<'_, Arc<Store>>,
    servers: Vec<proxy::ProxyServer>,
) -> AppResult<Vec<String>> {
    proxy::import(&state, servers)
}

/// 只读：转发 Codex Portal 本地 IPC 的 status，界面不提供切换。
#[tauri::command]
pub async fn portal_status() -> AppResult<Value> {
    portal::call(portal::Action::Status).await
}
