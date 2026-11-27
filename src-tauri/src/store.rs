use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type AppResult<T> = Result<T, String>;
pub fn now() -> String {
    Utc::now().to_rfc3339()
}
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn clipped(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    s.chars().take(max).collect::<String>() + "\n…（已截断）"
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub port: u16,
    pub cloudflared_path: String,
    pub tunnel_mode: String,
    pub tunnel_name: String,
    pub public_url: String,
    pub result_chars: usize,
    pub command_timeout_seconds: u64,
    pub codex_path: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8787,
            cloudflared_path: "cloudflared".into(),
            tunnel_mode: "quick".into(),
            tunnel_name: String::new(),
            public_url: String::new(),
            result_chars: 6000,
            command_timeout_seconds: 3600,
            codex_path: find_binary("codex"),
        }
    }
}
pub fn find_binary(name: &str) -> String {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths)
        .chain(dirs::home_dir().map(|home| home.join(".local/bin")))
        .chain([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ])
    {
        let p = dir.join(name);
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    name.to_owned()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scope {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Audit {
    pub id: i64,
    pub time: String,
    pub action: String,
    pub target: String,
    pub success: bool,
}
#[derive(Default)]
pub struct LiveState {
    pub server: Option<CancellationToken>,
    pub port: Option<u16>,
    pub tunnel: Option<CancellationToken>,
    pub tunnel_url: Option<String>,
    pub tunnel_error: Option<String>,
    pub server_id: Option<String>,
    pub tunnel_id: Option<String>,
}
pub struct Store {
    pub dir: PathBuf,
    pub db: Mutex<Connection>,
    pub settings: Mutex<Settings>,
    pub token: Mutex<String>,
    pub live: Mutex<LiveState>,
    pub roots: Mutex<HashMap<String, Arc<cap_std::fs::Dir>>>,
}
impl Store {
    pub fn open(dir: PathBuf) -> AppResult<Arc<Self>> {
        fs::create_dir_all(&dir).map_err(err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(err)?;
        }
        let db = Connection::open(dir.join("cloudmcp.sqlite3")).map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS scopes (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT UNIQUE NOT NULL, created_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS audit (id INTEGER PRIMARY KEY, time TEXT NOT NULL, action TEXT NOT NULL, target TEXT NOT NULL, success INTEGER NOT NULL);").map_err(err)?;
        crate::executions::init(&db)?;
        let settings = db
            .query_row("SELECT value FROM settings WHERE key='settings'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(err)?
            .unwrap_or_default();
        let token: String = db
            .query_row("SELECT value FROM settings WHERE key='token'", [], |r| {
                r.get(0)
            })
            .unwrap_or_else(|_| new_token());
        db.execute(
            "INSERT OR REPLACE INTO settings VALUES ('token', ?)",
            [&token],
        )
        .map_err(err)?;
        let store = Arc::new(Self {
            dir,
            db: Mutex::new(db),
            settings: Mutex::new(settings),
            token: Mutex::new(token),
            live: Mutex::new(LiveState::default()),
            roots: Mutex::new(HashMap::new()),
        });
        for scope in store.scopes()? {
            if let Ok(root) =
                cap_std::fs::Dir::open_ambient_dir(&scope.path, cap_std::ambient_authority())
            {
                store.roots.lock().unwrap().insert(scope.id, Arc::new(root));
            }
        }
        Ok(store)
    }
    pub fn config(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    pub fn save_settings(&self, s: Settings) -> AppResult<()> {
        if s.port < 1024 {
            return Err("请选择 1024–65535 之间的端口".into());
        }
        if !(1000..=16000).contains(&s.result_chars) {
            return Err("结果字符上限应为 1000–16000".into());
        }
        if !(1..=86400).contains(&s.command_timeout_seconds) {
            return Err("命令执行时限应为 1–86400 秒".into());
        }
        if s.codex_path.trim().is_empty() {
            return Err("Codex 可执行文件路径不能为空".into());
        }
        if !matches!(s.tunnel_mode.as_str(), "quick" | "named") {
            return Err("未知隧道模式".into());
        }
        if self.live.lock().unwrap().server.is_some() {
            return Err("请先停止连接，再修改设置".into());
        }
        self.db
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO settings VALUES ('settings', ?)",
                [serde_json::to_string(&s).map_err(err)?],
            )
            .map_err(err)?;
        *self.settings.lock().unwrap() = s;
        Ok(())
    }
    pub fn scopes(&self) -> AppResult<Vec<Scope>> {
        let db = self.db.lock().unwrap();
        let mut q = db
            .prepare("SELECT id,name,path,created_at FROM scopes ORDER BY created_at")
            .map_err(err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Scope {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
    pub fn scope(&self, name: &str) -> AppResult<Scope> {
        let scopes = self.scopes()?;
        if let Some(scope) = scopes.iter().find(|s| s.id == name) {
            return Ok(scope.clone());
        }
        let matches: Vec<_> = scopes.into_iter().filter(|s| s.name == name).collect();
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => Err("目录未授权或已撤销；请先调用 workspace_list 查询目录名称".into()),
            _ => Err("存在同名授权目录，请在本机为目录设置唯一名称".into()),
        }
    }
    pub fn add_scope(&self, path: &Path, name: Option<&str>) -> AppResult<Scope> {
        let path = path.canonicalize().map_err(err)?;
        if !path.is_dir() {
            return Err("请选择一个目录".into());
        }
        if self.dir.starts_with(&path) || path.starts_with(&self.dir) {
            return Err("授权目录不能包含 CloudMCP 自身的数据目录，请选择更具体的项目目录".into());
        }
        for s in self.scopes()? {
            let p = Path::new(&s.path);
            if p.starts_with(&path) || path.starts_with(p) {
                return Err("此目录已授权，或与现有授权目录重叠".into());
            }
        }
        let root =
            cap_std::fs::Dir::open_ambient_dir(&path, cap_std::ambient_authority()).map_err(err)?;
        let name = name.map(str::to_owned).unwrap_or_else(|| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        validate_scope_name(&name)?;
        let scope = Scope {
            id: Uuid::new_v4().to_string(),
            name,
            path: path.to_string_lossy().into_owned(),
            created_at: now(),
        };
        let inserted = self.db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO scopes SELECT ?,?,?,? WHERE NOT EXISTS (SELECT 1 FROM scopes WHERE name=?)",
                params![scope.id, scope.name, scope.path, scope.created_at, scope.name],
            )
            .map_err(err)?;
        if inserted == 0 {
            return Err("目录名称已使用，请设置不同的名称".into());
        }
        self.roots
            .lock()
            .unwrap()
            .insert(scope.id.clone(), Arc::new(root));
        self.audit("scope.add", &scope.path, true);
        Ok(scope)
    }
    pub fn rename_scope(&self, id: &str, name: &str) -> AppResult<()> {
        validate_scope_name(name)?;
        let scope = self.scope(id)?;
        let changed = self.db.lock().unwrap().execute(
            "UPDATE scopes SET name=? WHERE id=? AND NOT EXISTS (SELECT 1 FROM scopes WHERE name=? AND id<>?)",
            params![name, scope.id, name, scope.id],
        ).map_err(err)?;
        if changed == 0 {
            return Err("目录名称已使用，请设置不同的名称".into());
        }
        self.audit("scope.rename", name, true);
        Ok(())
    }
    pub fn remove_scope(&self, id: &str) -> AppResult<()> {
        let s = self.scope(id)?;
        crate::executions::cancel_scope(self, Some(&s.id), "目录授权已撤销")?;
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM scopes WHERE id=?", [&s.id])
            .map_err(err)?;
        self.roots.lock().unwrap().remove(&s.id);
        self.audit("scope.remove", &s.path, true);
        Ok(())
    }
    pub fn cancel_commands(&self) {
        let _ = crate::executions::cancel_scope(self, None, "本机停止服务或更新令牌");
    }
    pub fn audit(&self, action: &str, target: &str, success: bool) {
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO audit (time,action,target,success) VALUES (?,?,?,?)",
            params![now(), action, clipped(target, 240), success],
        );
        let _ = db.execute(
            "DELETE FROM audit WHERE id < (SELECT MAX(id)-1000 FROM audit)",
            [],
        );
    }
    pub fn audits(&self) -> AppResult<Vec<Audit>> {
        let db = self.db.lock().unwrap();
        let mut q = db
            .prepare("SELECT id,time,action,target,success FROM audit ORDER BY id DESC LIMIT 100")
            .map_err(err)?;
        let rows = q
            .query_map([], |r| {
                Ok(Audit {
                    id: r.get(0)?,
                    time: r.get(1)?,
                    action: r.get(2)?,
                    target: r.get(3)?,
                    success: r.get(4)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
    pub fn rotate_token(&self) -> AppResult<String> {
        let token = new_token();
        self.db
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO settings VALUES ('token',?)",
                [&token],
            )
            .map_err(err)?;
        *self.token.lock().unwrap() = token.clone();
        self.cancel_commands();
        self.audit("token.rotate", "", true);
        Ok(token)
    }
}
fn validate_scope_name(name: &str) -> AppResult<()> {
    if name.trim() != name
        || name.is_empty()
        || name.chars().count() > 80
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || matches!(name, "." | "..")
        || Uuid::parse_str(name).is_ok()
    {
        return Err("目录名称需为 1–80 个字符，不能包含路径分隔符、控制字符或使用 UUID".into());
    }
    Ok(())
}
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
