use crate::{
    privacy, server,
    store::{err, AppResult, Store},
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Config {
    data_dir: Option<String>,
    port: Option<u16>,
    token: Option<String>,
    command_timeout_seconds: Option<u64>,
    codex_path: Option<String>,
    #[serde(default)]
    directories: Vec<Directory>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Directory {
    Path(String),
    Named { path: String, name: String },
}
fn expand(path: &str) -> AppResult<PathBuf> {
    if path == "~" {
        return dirs::home_dir().ok_or_else(|| "无法解析主目录".into());
    }
    if let Some(tail) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|p| p.join(tail))
            .ok_or_else(|| "无法解析主目录".into());
    }
    Ok(PathBuf::from(path))
}
pub fn run(path: &str) -> AppResult<()> {
    let config: Config =
        serde_json::from_slice(&std::fs::read(expand(path)?).map_err(err)?).map_err(err)?;
    let data = match config.data_dir {
        Some(path) => expand(&path)?,
        None => dirs::data_dir()
            .ok_or("无法解析应用数据目录")?
            .join("dev.cloudmcp.desktop"),
    };
    let store = Store::open(data)?;
    if config.port.is_some()
        || config.command_timeout_seconds.is_some()
        || config.codex_path.is_some()
    {
        let mut settings = store.config();
        if let Some(port) = config.port {
            settings.port = port;
        }
        if let Some(timeout) = config.command_timeout_seconds {
            settings.command_timeout_seconds = timeout;
        }
        if let Some(path) = config.codex_path {
            settings.codex_path = expand(&path)?.to_string_lossy().into_owned();
        }
        store.save_settings(settings)?;
    }
    if let Some(token) = config.token {
        if token.len() < 32 || token.len() > 8192 || token.contains(['\n', '\r']) {
            return Err("配置中的访问令牌应至少 32 字节，且不能包含换行".into());
        }
        store
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO settings VALUES ('token',?)",
                [&token],
            )
            .map_err(err)?;
        *store.token.lock().unwrap() = token;
    }
    for directory in config.directories {
        let (path, name) = match directory {
            Directory::Path(p) => (p, None),
            Directory::Named { path, name } => (path, Some(name)),
        };
        let path = expand(&path)?.canonicalize().map_err(err)?;
        if let Some(scope) = store.scopes()?.iter().find(|s| Path::new(&s.path) == path) {
            if let Some(name) = name {
                if name != scope.name {
                    store.rename_scope(&scope.id, &name)?;
                }
            }
        } else {
            store.add_scope(&path, name.as_deref())?;
        }
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(err)?.block_on(async move {
        server::start(store.clone()).await?;
        eprintln!("CloudMCP: http://127.0.0.1:{}/mcp",store.config().port);
        eprintln!("Local data: {}",privacy::text(&store.dir.to_string_lossy()));
        #[cfg(unix)] {
            use tokio::signal::unix::{signal,SignalKind};
            let mut term=signal(SignalKind::terminate()).map_err(err)?;
            tokio::select!{result=tokio::signal::ctrl_c()=>result.map_err(err)?,_=term.recv()=>{}}
        }
        #[cfg(not(unix))] tokio::signal::ctrl_c().await.map_err(err)?;
        server::stop(&store);
        Ok(())
    })
}
