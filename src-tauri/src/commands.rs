use crate::{
    executions, portal, privacy, server,
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
        json!({"settings":settings,"scopes":privacy::value(json!(scopes)),"executions":privacy::value(json!(executions)),"audit":privacy::value(json!(audit)),"server_running":live.server.is_some(),"tunnel_running":live.tunnel.is_some(),"public_url":live.tunnel_url,"tunnel_error":live.tunnel_error.as_ref().map(|e|privacy::text(e)),"data_dir":privacy::text(&state.dir.to_string_lossy()),"platform":std::env::consts::OS}),
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
        checks.push(("codex", PathBuf::from(config.codex_path)));
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

/// 只读：转发 Codex Portal 本地 IPC 的 status，界面不提供切换。
#[tauri::command]
pub async fn portal_status() -> AppResult<Value> {
    portal::call(portal::Action::Status).await
}
