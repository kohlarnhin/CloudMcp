use crate::{
    files,
    store::{err, AppResult, Store},
};
use std::path::{Path, PathBuf};
use tokio::process::Command;
pub fn terminate(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
}

fn apple_developer_paths() -> Option<(PathBuf, PathBuf)> {
    // Match the system toolchain selected in the command's cleared environment.
    let output = std::process::Command::new("/usr/bin/xcode-select")
        .arg("--print-path")
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let selected = Path::new(
        std::str::from_utf8(&output.stdout)
            .ok()?
            .trim_end_matches(['\r', '\n']),
    );
    if !selected.is_absolute() {
        return None;
    }
    let developer = selected.canonicalize().ok()?;
    let tools = developer.join("usr").canonicalize().ok()?;
    (tools.is_dir() && tools != developer && tools.starts_with(&developer))
        .then_some((developer, tools))
}

pub fn command(
    store: &Store,
    workspace: &str,
    command: &str,
) -> AppResult<(Command, tempfile::TempDir)> {
    if !cfg!(target_os = "macos") {
        return Err("直接命令执行当前仅支持 macOS 的系统沙箱".into());
    }
    if command.trim().is_empty() || command.len() > 16000 {
        return Err("命令不能为空且不能超过 16 KB".into());
    }
    let scope = store.scope(workspace)?;
    files::root(store, &scope.id)?;
    let temp = tempfile::tempdir().map_err(err)?;
    let mut profile = "(version 1)\n(deny default)\n(allow process-exec process-fork sysctl-read mach-lookup)\n(allow signal process-info* (target same-sandbox))\n(allow file-read-metadata)\n(allow file-read* (literal \"/\"))\n(allow file-write-data (literal \"/dev/null\"))\n(allow network*)\n".to_owned();
    let mut cmd = Command::new("/usr/bin/sandbox-exec");
    for path in [
        "/System",
        "/usr",
        "/bin",
        "/sbin",
        "/Library/Apple",
        "/Library/Frameworks",
        "/opt/homebrew",
        "/private/etc",
        "/dev",
    ] {
        profile.push_str(&format!("(allow file-read* (subpath \"{path}\"))\n"));
    }
    if let Some((developer, tools)) = apple_developer_paths() {
        cmd.arg("-D")
            .arg(format!("APPLE_DEVELOPER_DIR={}", developer.display()))
            .arg("-D")
            .arg(format!("APPLE_DEVELOPER_USR={}", tools.display()));
        // Apple's /usr/bin/git uses xcrun, which reads the selected developer directory and its SDKs.
        profile.push_str(
            "(allow file-read* (subpath (param \"APPLE_DEVELOPER_DIR\")))\n\
             (allow file-read* (subpath (param \"APPLE_DEVELOPER_USR\")))\n\
             (allow file-read* (subpath \"/private/var/db/xcode_select_link\"))\n\
             (allow file-read* (subpath \"/private/var/select\"))\n",
        );
    }
    let scopes = store.scopes()?;
    for (index, root) in scopes.iter().enumerate() {
        let path = Path::new(&root.path);
        if path.canonicalize().map_err(err)? != path {
            return Err("授权目录路径已改变，请在本机重新授权".into());
        }
        cmd.args(["-D", &format!("ROOT_{index}={}", root.path)]);
        profile.push_str(&format!(
            "(allow file-read* file-write* (subpath (param \"ROOT_{index}\")))\n"
        ));
        profile.push_str(&format!(
            "(deny file-write-unlink (literal (param \"ROOT_{index}\")))\n"
        ));
    }
    cmd.args([
        "-D",
        &format!(
            "COMMAND_TMP={}",
            temp.path().canonicalize().map_err(err)?.display()
        ),
    ]);
    profile.push_str("(allow file-read* file-write* (subpath (param \"COMMAND_TMP\")))\n");
    let toolchain = if store.config().codex_enabled {
        None
    } else {
        Some(toolchain_access(&mut cmd, &mut profile))
    };
    cmd.args(["-p", &profile, "/bin/sh", "-c", command])
        .env_clear()
        .env("TMPDIR", temp.path())
        .env("LANG", "en_US.UTF-8")
        .current_dir(&scope.path);
    match toolchain {
        Some(toolchain) => {
            cmd.env("HOME", &toolchain.home)
                .env("PATH", &toolchain.path);
            for (key, value) in toolchain.env {
                cmd.env(key, value);
            }
        }
        None => {
            cmd.env("HOME", &scope.path).env("PATH", BASE_PATH);
        }
    }
    Ok((cmd, temp))
}

const BASE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Read-only toolchain locations, relative to the user's home directory.
const HOME_READ: &[&str] = &[
    ".gitconfig",
    ".gitignore_global",
    ".config",
    ".ssh",
    ".local",
    ".nvm",
    ".volta",
    ".npmrc",
    ".yarnrc",
    "Library/Keychains",
    "Library/Preferences",
    "Library/Application Support/fnm",
];
/// Toolchain homes and caches that builds update, relative to the user's home directory.
const HOME_WRITE: &[&str] = &[
    ".cargo",
    ".rustup",
    ".npm",
    ".pnpm-store",
    ".yarn",
    ".bun",
    ".cache",
    ".local/share/pnpm",
    ".local/state",
    ".ssh/known_hosts",
    "Library/pnpm",
    "Library/Caches",
];
/// Toolchain bin directories placed before the base PATH, relative to the user's home directory.
const HOME_BIN: &[&str] = &[
    ".cargo/bin",
    ".local/bin",
    "Library/pnpm",
    ".bun/bin",
    ".volta/bin",
];

struct Toolchain {
    home: PathBuf,
    path: String,
    env: Vec<(&'static str, String)>,
}

fn darwin_dir(name: libc::c_int) -> Option<PathBuf> {
    let mut buf = vec![0 as libc::c_char; 1024];
    let len = unsafe { libc::confstr(name, buf.as_mut_ptr(), buf.len()) };
    if len == 0 || len > buf.len() {
        return None;
    }
    let text = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_str()
        .ok()?;
    PathBuf::from(text).canonicalize().ok()
}

/// With Codex delegation turned off the calling agent implements changes itself, so ordinary
/// commands get the developer toolchain Codex would otherwise provide: the real HOME for Git and
/// SSH configuration, Rust and Node toolchains with their caches, Xcode, and the per-user
/// temporary and cache directories that xcrun, cargo and package managers write to.
/// Writes outside authorized directories stay limited to toolchain homes and caches.
fn toolchain_access(cmd: &mut Command, profile: &mut String) -> Toolchain {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let mut index = 0usize;
    let mut allow = |cmd: &mut Command, profile: &mut String, path: &Path, write: bool| {
        let Ok(path) = path.canonicalize() else {
            return;
        };
        let key = format!("TOOL_{index}");
        index += 1;
        cmd.args(["-D", &format!("{key}={}", path.display())]);
        let filter = if path.is_dir() { "subpath" } else { "literal" };
        let rights = if write {
            "file-read* file-write*"
        } else {
            "file-read*"
        };
        profile.push_str(&format!("(allow {rights} ({filter} (param \"{key}\")))\n"));
    };
    for path in [
        "/Library/Developer",
        "/Applications/Xcode.app",
        "/Applications/Xcode-beta.app",
        "/Library/Java",
    ] {
        allow(cmd, profile, Path::new(path), false);
    }
    for rel in HOME_READ {
        allow(cmd, profile, &home.join(rel), false);
    }
    // Git config often includes sibling files such as ~/.gitconfig-work through includeIf.
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name.starts_with(".gitconfig") || name.starts_with(".gitignore"))
                && !HOME_READ.contains(&name.as_ref())
            {
                allow(cmd, profile, &entry.path(), false);
            }
        }
    }
    for rel in HOME_WRITE {
        allow(cmd, profile, &home.join(rel), true);
    }
    for dir in [
        libc::_CS_DARWIN_USER_TEMP_DIR,
        libc::_CS_DARWIN_USER_CACHE_DIR,
    ]
    .into_iter()
    .filter_map(darwin_dir)
    {
        allow(cmd, profile, &dir, true);
    }
    allow(cmd, profile, Path::new("/private/tmp"), true);
    let mut env = Vec::new();
    for key in ["USER", "LOGNAME", "SHELL", "SSH_AUTH_SOCK"] {
        if let Ok(value) = std::env::var(key) {
            if key == "SSH_AUTH_SOCK" {
                allow(cmd, profile, Path::new(&value), true);
            }
            env.push((key, value));
        }
    }
    let path = HOME_BIN
        .iter()
        .map(|rel| home.join(rel))
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .chain([BASE_PATH.to_owned()])
        .collect::<Vec<_>>()
        .join(":");
    Toolchain { home, path, env }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::command;
    use crate::store::Store;

    #[tokio::test]
    async fn apple_git_can_initialize_a_repository_in_the_sandbox() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        store.add_scope(&workspace, Some("git-test")).unwrap();

        let (mut cmd, _command_temp) =
            command(&store, "git-test", "/usr/bin/git init --quiet").unwrap();
        let output = cmd.output().await.unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(workspace.join(".git/HEAD").is_file());
    }

    fn env_of(cmd: &tokio::process::Command, key: &str) -> Option<String> {
        cmd.as_std()
            .get_envs()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[tokio::test]
    async fn codex_disabled_commands_get_the_developer_toolchain() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        store.add_scope(&workspace, Some("tool-test")).unwrap();

        let (cmd, _sandbox_temp) = command(&store, "tool-test", "true").unwrap();
        assert_eq!(
            env_of(&cmd, "HOME").unwrap(),
            workspace.to_string_lossy().as_ref()
        );

        let mut settings = store.config();
        settings.local_agent = "none".into();
        store.save_settings(settings).unwrap();
        let (mut cmd, _sandbox_temp) = command(
            &store,
            "tool-test",
            "/usr/bin/git --version && /usr/bin/git config --global --list >/dev/null && /usr/bin/git status --short >/dev/null 2>&1 || /usr/bin/git init --quiet",
        )
        .unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            env_of(&cmd, "HOME").unwrap(),
            home.to_string_lossy().as_ref()
        );
        assert!(env_of(&cmd, "PATH").unwrap().ends_with("/usr/sbin:/sbin"));
        let output = cmd.output().await.unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert!(!stderr.contains("xcrun_db"), "{stderr}");
    }
}
