//! Local MCP servers that CloudMCP connects to on this machine and forwards to remote agents.
//!
//! Remote agents cannot reach stdio servers or localhost endpoints on this Mac. CloudMCP acts as
//! their MCP client and offers them through three gateway tools (proxy_list, proxy_tools,
//! proxy_call), so the remote tool list stays small however many servers are configured.
use crate::store::{err, AppResult, Store};
use axum::http::{HeaderName, HeaderValue};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, JsonObject, Tool},
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        TokioChildProcess,
    },
    RoleClient, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::process::Command;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
const IDLE_LIMIT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct ProxyServer {
    /// Unique name: letters, digits, `_` and `-`.
    pub name: String,
    pub enabled: bool,
    /// `stdio` or `http` (streamable HTTP).
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    /// What the server offers, in a sentence, shown to remote agents.
    pub description: String,
    /// Where it was imported from, e.g. Pi or Claude Desktop.
    pub source: String,
}

struct Live {
    service: RunningService<RoleClient, ()>,
    config: ProxyServer,
    tools: Mutex<Option<Vec<Tool>>>,
    last_used: Mutex<Instant>,
}

#[derive(Default)]
pub struct Pool {
    live: Mutex<HashMap<String, Arc<Live>>>,
    reaper: std::sync::OnceLock<()>,
}

// ---------- configuration ----------

pub fn list(store: &Store) -> AppResult<Vec<ProxyServer>> {
    let raw: Option<String> = store
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM settings WHERE key='proxy_servers'",
            [],
            |r| r.get(0),
        )
        .ok();
    match raw {
        Some(raw) => serde_json::from_str(&raw).map_err(err),
        None => Ok(Vec::new()),
    }
}
fn write(store: &Store, servers: &[ProxyServer]) -> AppResult<()> {
    store
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO settings VALUES ('proxy_servers', ?)",
            [serde_json::to_string(servers).map_err(err)?],
        )
        .map_err(err)?;
    Ok(())
}
pub fn enabled(store: &Store) -> Vec<ProxyServer> {
    list(store)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.enabled)
        .collect()
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
/// Turns a name from another client's configuration into a valid proxy name.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    if cleaned.is_empty() {
        "server".into()
    } else {
        cleaned
    }
}

fn validate(server: &mut ProxyServer) -> AppResult<()> {
    server.name = server.name.trim().to_owned();
    if !valid_name(&server.name) {
        return Err("名称需为 1–48 个字母、数字、下划线或连字符".into());
    }
    match server.transport.as_str() {
        "stdio" => {
            server.command = server.command.trim().to_owned();
            if server.command.is_empty() {
                return Err("stdio 服务器需要填写启动命令".into());
            }
            server.url.clear();
            server.headers.clear();
        }
        "http" => {
            server.url = server.url.trim().to_owned();
            if !(server.url.starts_with("https://") || server.url.starts_with("http://")) {
                return Err("HTTP 服务器需要填写 http(s) 地址".into());
            }
            server.command.clear();
            server.args.clear();
            server.env.clear();
            server.cwd.clear();
        }
        _ => return Err("传输方式应为 stdio 或 http".into()),
    }
    if server.description.chars().count() > 300 {
        return Err("说明最多 300 个字符".into());
    }
    Ok(())
}

/// Adds or replaces a server. `original` is the name being edited, if any.
pub fn save(store: &Store, mut server: ProxyServer, original: Option<&str>) -> AppResult<()> {
    validate(&mut server)?;
    let mut servers = list(store)?;
    let target = original.unwrap_or(&server.name).to_owned();
    if servers
        .iter()
        .any(|s| s.name == server.name && s.name != target)
    {
        return Err("已有同名 MCP 服务器".into());
    }
    match servers.iter_mut().find(|s| s.name == target) {
        Some(existing) => *existing = server.clone(),
        None => servers.push(server.clone()),
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    write(store, &servers)?;
    disconnect(store, &target);
    disconnect(store, &server.name);
    store.audit("proxy.save", &server.name, true);
    Ok(())
}
pub fn remove(store: &Store, name: &str) -> AppResult<()> {
    let mut servers = list(store)?;
    servers.retain(|s| s.name != name);
    write(store, &servers)?;
    disconnect(store, name);
    store.audit("proxy.remove", name, true);
    Ok(())
}
pub fn set_enabled(store: &Store, name: &str, enabled: bool) -> AppResult<()> {
    let mut servers = list(store)?;
    let server = servers
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or("找不到该 MCP 服务器")?;
    server.enabled = enabled;
    write(store, &servers)?;
    if !enabled {
        disconnect(store, name);
    }
    Ok(())
}

// ---------- environment ----------

/// The login shell environment, so servers see the PATH and tokens from the user's shell profile
/// even when CloudMCP was opened from Finder.
fn login_env() -> &'static HashMap<String, String> {
    static ENV: OnceLock<HashMap<String, String>> = OnceLock::new();
    ENV.get_or_init(|| {
        let mut env: HashMap<String, String> = std::env::vars().collect();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        if let Some(stdout) = shell_env(&shell) {
            {
                for entry in stdout.split(|b| *b == 0) {
                    let entry = String::from_utf8_lossy(entry);
                    if let Some((key, value)) = entry.split_once('=') {
                        if !key.is_empty() && !key.contains(char::is_whitespace) {
                            env.insert(key.to_owned(), value.to_owned());
                        }
                    }
                }
            }
        }
        let home = dirs::home_dir().unwrap_or_default();
        let mut paths: Vec<PathBuf> = env
            .get("PATH")
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        for extra in [
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join(".bun/bin"),
            home.join(".volta/bin"),
            home.join("Library/pnpm"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ] {
            if !paths.contains(&extra) {
                paths.push(extra);
            }
        }
        if let Ok(path) = std::env::join_paths(paths) {
            env.insert("PATH".into(), path.to_string_lossy().into_owned());
        }
        env
    })
}

/// Runs the login shell once to read its environment, giving up after five seconds.
fn shell_env(shell: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut child = std::process::Command::new(shell)
        .args(["-l", "-i", "-c", "env -0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return reader.join().ok(),
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Expands `${VAR}` from the login environment, a leading `~/`, and a whole-value `!command`
/// (its trimmed stdout), the conventions shared by Pi, Claude and Cursor configurations.
fn expand(value: &str) -> AppResult<String> {
    let env = login_env();
    if let Some(command) = value.strip_prefix('!') {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", command])
            .envs(env)
            .stdin(Stdio::null())
            .output()
            .map_err(err)?;
        if !output.status.success() {
            return Err(format!("取值命令执行失败：{command}"));
        }
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let key = &after[..end];
        let resolved = env
            .get(key)
            .ok_or_else(|| format!("环境变量 {key} 未设置"))?;
        out.push_str(resolved);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    if let Some(tail) = out.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join(tail).to_string_lossy().into_owned());
        }
    }
    Ok(out)
}

// ---------- connections ----------

async fn connect(config: &ProxyServer) -> AppResult<RunningService<RoleClient, ()>> {
    let service = match config.transport.as_str() {
        "stdio" => {
            let env = login_env();
            let mut cmd = Command::new(expand(&config.command)?);
            for arg in &config.args {
                cmd.arg(expand(arg)?);
            }
            cmd.env_clear().envs(env);
            for (key, value) in &config.env {
                cmd.env(key, expand(value)?);
            }
            let cwd = if config.cwd.trim().is_empty() {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
            } else {
                PathBuf::from(expand(config.cwd.trim())?)
            };
            cmd.current_dir(cwd);
            let (transport, _) = TokioChildProcess::builder(cmd)
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("无法启动 {}：{e}", config.command))?;
            tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport))
                .await
                .map_err(|_| "连接超时".to_owned())?
                .map_err(|e| format!("初始化失败：{e}"))?
        }
        _ => {
            let mut headers = HashMap::new();
            for (key, value) in &config.headers {
                let name = HeaderName::from_bytes(key.trim().as_bytes())
                    .map_err(|_| format!("无效的请求头名称：{key}"))?;
                let value = HeaderValue::from_str(&expand(value)?)
                    .map_err(|_| format!("请求头 {key} 的值无效"))?;
                headers.insert(name, value);
            }
            let transport = StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(expand(&config.url)?)
                    .custom_headers(headers),
            );
            tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport))
                .await
                .map_err(|_| "连接超时".to_owned())?
                .map_err(|e| format!("初始化失败：{e}"))?
        }
    };
    Ok(service)
}

fn start_reaper(store: &Arc<Store>) {
    let pool = &store.proxy;
    let weak = Arc::downgrade(store);
    if pool.reaper.set(()).is_ok() {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(store) = weak.upgrade() else { return };
                let mut live = store.proxy.live.lock().unwrap();
                live.retain(|_, c| {
                    !c.service.is_closed() && c.last_used.lock().unwrap().elapsed() < IDLE_LIMIT
                });
            }
        });
    }
}

pub fn disconnect(store: &Store, name: &str) {
    store.proxy.live.lock().unwrap().remove(name);
}
pub fn disconnect_all(store: &Store) {
    store.proxy.live.lock().unwrap().clear();
}
pub fn connected(store: &Store, name: &str) -> bool {
    store
        .proxy
        .live
        .lock()
        .unwrap()
        .get(name)
        .is_some_and(|c| !c.service.is_closed())
}

async fn session(store: &Arc<Store>, name: &str, require_enabled: bool) -> AppResult<Arc<Live>> {
    let config = list(store)?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("没有名为 {name} 的 MCP 服务器；请先调用 proxy_list"))?;
    if require_enabled && !config.enabled {
        return Err(format!("MCP 服务器 {name} 已在本机停用"));
    }
    if let Some(live) = store.proxy.live.lock().unwrap().get(name).cloned() {
        if !live.service.is_closed() && live.config == config {
            *live.last_used.lock().unwrap() = Instant::now();
            return Ok(live);
        }
    }
    start_reaper(store);
    let service = connect(&config).await?;
    let live = Arc::new(Live {
        service,
        config,
        tools: Mutex::new(None),
        last_used: Mutex::new(Instant::now()),
    });
    store
        .proxy
        .live
        .lock()
        .unwrap()
        .insert(name.to_owned(), live.clone());
    Ok(live)
}

async fn tools_of(live: &Live) -> AppResult<Vec<Tool>> {
    if let Some(tools) = live.tools.lock().unwrap().clone() {
        return Ok(tools);
    }
    let tools = tokio::time::timeout(CONNECT_TIMEOUT, live.service.list_all_tools())
        .await
        .map_err(|_| "读取工具列表超时".to_owned())?
        .map_err(|e| format!("读取工具列表失败：{e}"))?;
    *live.tools.lock().unwrap() = Some(tools.clone());
    Ok(tools)
}

// ---------- gateway operations ----------

pub fn overview(store: &Store) -> AppResult<Value> {
    let servers: Vec<_> = enabled(store)
        .into_iter()
        .map(|s| {
            let cached = store
                .proxy
                .live
                .lock()
                .unwrap()
                .get(&s.name)
                .and_then(|c| c.tools.lock().unwrap().as_ref().map(Vec::len));
            json!({"server":s.name,"transport":s.transport,"description":s.description,"connected":connected(store,&s.name),"tool_count":cached})
        })
        .collect();
    Ok(json!({"servers":servers}))
}

pub async fn tools(store: &Arc<Store>, server: &str) -> AppResult<Value> {
    let live = session(store, server, true).await?;
    let tools = match tools_of(&live).await {
        Ok(tools) => tools,
        Err(e) => {
            disconnect(store, server);
            return Err(e);
        }
    };
    let entries: Vec<_> = tools
        .iter()
        .map(|t| json!({"name":t.name,"description":t.description,"input_schema":t.input_schema,"annotations":t.annotations}))
        .collect();
    Ok(json!({"server":server,"tools":entries}))
}

pub async fn call(
    store: &Arc<Store>,
    server: &str,
    tool: &str,
    arguments: Option<JsonObject>,
) -> AppResult<CallToolResult> {
    let live = session(store, server, true).await?;
    let mut params = CallToolRequestParams::new(tool.to_owned());
    if let Some(arguments) = arguments {
        params = params.with_arguments(arguments);
    }
    let result = tokio::time::timeout(CALL_TIMEOUT, live.service.call_tool(params)).await;
    *live.last_used.lock().unwrap() = Instant::now();
    match result {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(e)) => {
            if live.service.is_closed() {
                disconnect(store, server);
            }
            Err(format!("调用 {server}/{tool} 失败：{e}"))
        }
        Err(_) => Err(format!(
            "调用 {server}/{tool} 超过 {} 秒",
            CALL_TIMEOUT.as_secs()
        )),
    }
}

/// Connects (reusing a live connection) and lists tool names, for the local settings page.
pub async fn test(store: &Arc<Store>, name: &str) -> AppResult<Value> {
    disconnect(store, name);
    let live = session(store, name, false).await?;
    let tools = tools_of(&live).await;
    if tools.is_err() {
        disconnect(store, name);
    }
    let names: Vec<_> = tools?.into_iter().map(|t| t.name.to_string()).collect();
    Ok(json!({"server":name,"tools":names}))
}

// ---------- import ----------

#[derive(Serialize)]
pub struct ImportSource {
    pub source: String,
    pub path: String,
    pub servers: Vec<ProxyServer>,
    pub skipped: Vec<String>,
    pub error: Option<String>,
}

fn string_map(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads the common `mcpServers` JSON shape used by Pi, Claude Desktop, Claude Code and Cursor.
fn parse_json_servers(value: &Value, source: &str, skipped: &mut Vec<String>) -> Vec<ProxyServer> {
    let Some(entries) = value.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut servers = Vec::new();
    for (name, entry) in entries {
        let kind = entry["type"].as_str().unwrap_or_default();
        if kind == "sse" {
            skipped.push(format!("{name}（SSE 传输不支持）"));
            continue;
        }
        let mut server = ProxyServer {
            name: sanitize_name(name),
            enabled: entry["enabled"].as_bool().unwrap_or(true),
            description: entry["description"].as_str().unwrap_or_default().to_owned(),
            source: source.to_owned(),
            ..Default::default()
        };
        if let Some(command) = entry["command"].as_str() {
            server.transport = "stdio".into();
            server.command = command.to_owned();
            server.args = entry["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            server.env = string_map(&entry["env"]);
            server.cwd = entry["cwd"].as_str().unwrap_or_default().to_owned();
        } else if let Some(url) = entry["url"]
            .as_str()
            .or_else(|| entry["serverUrl"].as_str())
        {
            server.transport = "http".into();
            server.url = url.to_owned();
            server.headers = string_map(&entry["headers"]);
        } else {
            skipped.push(format!("{name}（缺少 command 或 url）"));
            continue;
        }
        servers.push(server);
    }
    servers
}

/// Reads `[mcp_servers.<name>]` tables from a Codex config.toml.
fn parse_codex(text: &str, skipped: &mut Vec<String>) -> AppResult<Vec<ProxyServer>> {
    let value: toml::Value = toml::from_str(text).map_err(err)?;
    let Some(entries) = value.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let text_of = |v: Option<&toml::Value>| v.and_then(toml::Value::as_str).map(str::to_owned);
    let map_of = |v: Option<&toml::Value>| -> BTreeMap<String, String> {
        v.and_then(toml::Value::as_table)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut servers = Vec::new();
    for (name, entry) in entries {
        let mut server = ProxyServer {
            name: sanitize_name(name),
            enabled: entry
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            source: "Codex".into(),
            ..Default::default()
        };
        if let Some(command) = text_of(entry.get("command")) {
            server.transport = "stdio".into();
            server.command = command;
            server.args = entry
                .get("args")
                .and_then(toml::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            server.env = map_of(entry.get("env"));
            server.cwd = text_of(entry.get("cwd")).unwrap_or_default();
        } else if let Some(url) = text_of(entry.get("url")) {
            server.transport = "http".into();
            server.url = url;
            server.headers = map_of(entry.get("http_headers"));
            if let Some(var) = text_of(entry.get("bearer_token_env_var")) {
                server
                    .headers
                    .insert("Authorization".into(), format!("Bearer ${{{var}}}"));
            }
        } else {
            skipped.push(format!("{name}（缺少 command 或 url）"));
            continue;
        }
        servers.push(server);
    }
    Ok(servers)
}

fn read_source(source: &str, path: PathBuf) -> Option<ImportSource> {
    if !path.is_file() {
        return None;
    }
    let mut skipped = Vec::new();
    let result = std::fs::read_to_string(&path)
        .map_err(err)
        .and_then(|text| {
            if path.extension().is_some_and(|e| e == "toml") {
                parse_codex(&text, &mut skipped)
            } else {
                serde_json::from_str::<Value>(&text)
                    .map_err(err)
                    .map(|v| parse_json_servers(&v, source, &mut skipped))
            }
        });
    let (servers, error) = match result {
        Ok(servers) => (servers, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    Some(ImportSource {
        source: source.into(),
        path: crate::privacy::text(&path.to_string_lossy()),
        servers,
        skipped,
        error,
    })
}

/// MCP configurations of other clients found on this machine.
pub fn import_sources() -> Vec<ImportSource> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        ("Pi", home.join(".pi/agent/mcp.json")),
        (
            "Claude Desktop",
            home.join("Library/Application Support/Claude/claude_desktop_config.json"),
        ),
        ("Claude Code", home.join(".claude.json")),
        ("Cursor", home.join(".cursor/mcp.json")),
        ("Codex", home.join(".codex/config.toml")),
    ]
    .into_iter()
    .filter_map(|(source, path)| read_source(source, path))
    .collect()
}

/// Adds imported servers; an existing name gets a numeric suffix instead of being replaced.
pub fn import(store: &Store, servers: Vec<ProxyServer>) -> AppResult<Vec<String>> {
    let mut existing = list(store)?;
    let mut added = Vec::new();
    for mut server in servers {
        validate(&mut server)?;
        let base = server.name.clone();
        let mut n = 2;
        while existing.iter().any(|s| s.name == server.name) {
            server.name = format!("{}-{n}", base.chars().take(44).collect::<String>());
            n += 1;
        }
        added.push(server.name.clone());
        existing.push(server);
    }
    existing.sort_by(|a, b| a.name.cmp(&b.name));
    write(store, &existing)?;
    store.audit("proxy.import", &added.join(","), true);
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_json_and_codex_shapes() {
        let mut skipped = Vec::new();
        let value = serde_json::json!({"mcpServers":{
            "file system":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","."],"env":{"A":"${B}"}},
            "docs":{"url":"https://example.com/mcp","headers":{"Authorization":"Bearer ${DOCS_TOKEN}"},"description":"Docs"},
            "old":{"type":"sse","url":"https://example.com/sse"},
            "broken":{}
        }});
        let servers = parse_json_servers(&value, "Pi", &mut skipped);
        assert_eq!(servers.len(), 2);
        let fs = servers.iter().find(|s| s.name == "file-system").unwrap();
        assert_eq!(fs.transport, "stdio");
        assert_eq!(fs.args.len(), 3);
        let docs = servers.iter().find(|s| s.name == "docs").unwrap();
        assert_eq!(docs.transport, "http");
        assert_eq!(docs.description, "Docs");
        assert_eq!(skipped.len(), 2);

        let codex = r#"
[mcp_servers.github]
url = "https://api.githubcopilot.com/mcp/"
bearer_token_env_var = "GITHUB_TOKEN"
[mcp_servers.local]
command = "uvx"
args = ["tools-mcp"]
env = { KEY = "v" }
"#;
        let mut skipped = Vec::new();
        let servers = parse_codex(codex, &mut skipped).unwrap();
        assert_eq!(servers.len(), 2);
        let github = servers.iter().find(|s| s.name == "github").unwrap();
        assert_eq!(github.headers["Authorization"], "Bearer ${GITHUB_TOKEN}");
        assert_eq!(
            servers.iter().find(|s| s.name == "local").unwrap().env["KEY"],
            "v"
        );
    }

    #[test]
    fn saving_validates_and_import_renames_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        let server = ProxyServer {
            name: "fs".into(),
            enabled: true,
            transport: "stdio".into(),
            command: "npx".into(),
            ..Default::default()
        };
        save(&store, server.clone(), None).unwrap();
        assert!(save(
            &store,
            ProxyServer {
                name: "bad name".into(),
                ..server.clone()
            },
            None
        )
        .is_err());
        assert!(save(
            &store,
            ProxyServer {
                transport: "http".into(),
                url: "ftp://x".into(),
                ..server.clone()
            },
            None
        )
        .is_err());
        let added = import(&store, vec![server.clone()]).unwrap();
        assert_eq!(added, vec!["fs-2".to_string()]);
        set_enabled(&store, "fs-2", false).unwrap();
        assert_eq!(enabled(&store).len(), 1);
        remove(&store, "fs").unwrap();
        assert_eq!(list(&store).unwrap().len(), 1);
    }

    #[test]
    fn expands_variables_and_home() {
        std::env::set_var("CLOUDMCP_PROXY_TEST", "abc");
        // login_env snapshots the process environment on first use.
        assert_eq!(expand("x-${CLOUDMCP_PROXY_TEST}-y").unwrap(), "x-abc-y");
        assert!(expand("${CLOUDMCP_PROXY_MISSING_VAR}").is_err());
        assert!(expand("~/a").unwrap().ends_with("/a"));
        assert_eq!(expand("!echo hi").unwrap(), "hi");
    }

    const MOCK_SERVER: &str = r#"
const rl = require('readline').createInterface({ input: process.stdin });
const send = (m) => process.stdout.write(JSON.stringify(m) + '\n');
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  if (m.method === 'initialize') send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'mock', version: '1.0.0' } } });
  else if (m.method === 'tools/list') send({ jsonrpc: '2.0', id: m.id, result: { tools: [{ name: 'echo', description: 'Echo text', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } }] } });
  else if (m.method === 'tools/call') send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: 'echo:' + m.params.arguments.text + ':' + process.env.MOCK_SUFFIX }] } });
  else send({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'unknown' } });
});
"#;

    #[tokio::test]
    async fn forwards_list_and_call_to_a_stdio_server() {
        if std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("node not found; skipping");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("mock.js");
        std::fs::write(&script, MOCK_SERVER).unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        save(
            &store,
            ProxyServer {
                name: "mock".into(),
                enabled: true,
                transport: "stdio".into(),
                command: "node".into(),
                args: vec![script.to_string_lossy().into()],
                env: BTreeMap::from([("MOCK_SUFFIX".into(), "ok".into())]),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let tested = test(&store, "mock").await.unwrap();
        assert_eq!(tested["tools"], serde_json::json!(["echo"]));
        let listed = tools(&store, "mock").await.unwrap();
        assert_eq!(listed["tools"][0]["name"], "echo");
        let mut args = JsonObject::new();
        args.insert("text".into(), "hi".into());
        let result = call(&store, "mock", "echo", Some(args)).await.unwrap();
        let text = serde_json::to_value(&result.content).unwrap();
        assert_eq!(text[0]["text"], "echo:hi:ok");
        assert!(connected(&store, "mock"));
        assert_eq!(overview(&store).unwrap()["servers"][0]["connected"], true);
        disconnect_all(&store);
        assert!(!connected(&store, "mock"));
    }
}
