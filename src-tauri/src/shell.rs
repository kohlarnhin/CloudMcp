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
        // Apple's /usr/bin/git uses xcrun and needs to read the selected directory itself.
        profile.push_str(
            "(allow file-read* (literal (param \"APPLE_DEVELOPER_DIR\")))\n\
             (allow file-read* (subpath (param \"APPLE_DEVELOPER_USR\")))\n",
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
    cmd.args(["-p", &profile, "/bin/sh", "-c", command])
        .env_clear()
        .env("HOME", &scope.path)
        .env("TMPDIR", temp.path())
        .env("LANG", "en_US.UTF-8")
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .current_dir(&scope.path);
    Ok((cmd, temp))
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
}
