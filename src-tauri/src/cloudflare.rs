//! Cloudflare account login and automatic named-tunnel setup.
//!
//! cloudflared keeps its login certificate in `$HOME/.cloudflared/cert.pem`. CloudMCP runs every
//! cloudflared management command with HOME pointed at its own data directory, so the login and
//! tunnel credentials stay private to CloudMCP and never touch the user's `~/.cloudflared`.
use crate::{
    store::{err, AppResult, Store},
    tunnel,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

/// How long the browser authorization may take before the login is abandoned.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);

/// The directory cloudflared treats as HOME for CloudMCP's own Cloudflare state.
pub fn home(store: &Store) -> PathBuf {
    store.dir.join("cloudflare")
}
pub fn cert_path(store: &Store) -> PathBuf {
    home(store).join(".cloudflared").join("cert.pem")
}
/// Credentials of a tunnel CloudMCP created itself, keyed by tunnel name.
pub fn credentials_path(store: &Store, tunnel_name: &str) -> PathBuf {
    home(store)
        .join("tunnels")
        .join(format!("{tunnel_name}.json"))
}
pub fn logged_in(store: &Store) -> bool {
    cert_path(store).is_file()
}
/// The tunnel ID inside a credentials file CloudMCP created, if any.
pub fn managed_tunnel_id(store: &Store, tunnel_name: &str) -> Option<String> {
    let text = std::fs::read_to_string(credentials_path(store, tunnel_name)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value["TunnelID"].as_str().map(str::to_owned)
}

fn private_dir(path: &Path) -> AppResult<()> {
    std::fs::create_dir_all(path).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(())
}

fn cloudflared(store: &Store) -> Command {
    let mut cmd = Command::new(tunnel::executable(&store.config().cloudflared_path));
    cmd.env("HOME", home(store))
        .env_remove("TUNNEL_ORIGIN_CERT")
        .env_remove("TUNNEL_CRED_FILE")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    cmd
}

/// Runs a cloudflared management command and returns its combined output.
async fn run(store: &Store, args: &[&str]) -> AppResult<String> {
    let mut cmd = cloudflared(store);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = tokio::time::timeout(COMMAND_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "cloudflared 响应超时，请检查网络后重试".to_owned())?
        .map_err(|e| format!("无法启动 cloudflared：{e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text)
    } else {
        Err(last_error(&text))
    }
}

/// cloudflared prints timestamped log lines; keep the most useful error for the UI.
fn last_error(text: &str) -> String {
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("cloudflared 执行失败");
    crate::store::clipped(line, 400)
}

pub fn status(store: &Store) -> Value {
    let config = store.config();
    let managed = managed_tunnel_id(store, &config.tunnel_name).is_some();
    json!({ "logged_in": logged_in(store), "managed_tunnel": managed })
}

/// Opens the Cloudflare authorization page in the browser and waits for the login certificate.
pub async fn login(store: &Store) -> AppResult<()> {
    if logged_in(store) {
        return Ok(());
    }
    private_dir(&home(store))?;
    let mut cmd = cloudflared(store);
    cmd.args(["tunnel", "login"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 cloudflared：{e}"))?;
    let output = tokio::time::timeout(LOGIN_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "等待浏览器授权超时，请重新登录".to_owned())?
        .map_err(err)?;
    if !logged_in(store) {
        let text = String::from_utf8_lossy(&output.stderr);
        store.audit("cloudflare.login", "", false);
        return Err(format!("Cloudflare 登录未完成：{}", last_error(&text)));
    }
    store.audit("cloudflare.login", "", true);
    Ok(())
}

pub fn logout(store: &Store) -> AppResult<()> {
    let cert = cert_path(store);
    if cert.exists() {
        std::fs::remove_file(cert).map_err(err)?;
    }
    store.audit("cloudflare.logout", "", true);
    Ok(())
}

/// Validates a fully qualified hostname such as `mcp.example.com`.
pub fn normalize_hostname(input: &str) -> AppResult<String> {
    let host = input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_end_matches("/mcp")
        .to_ascii_lowercase();
    let labels: Vec<_> = host.split('.').collect();
    let valid = host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !valid {
        return Err("请填写完整的域名，例如 mcp.example.com".into());
    }
    Ok(host)
}

/// The tunnel name CloudMCP uses for a hostname: `cloudmcp-mcp-example-com`.
pub fn tunnel_name(host: &str) -> String {
    format!("cloudmcp-{}", host.replace('.', "-"))
}

/// Creates (or reuses) a named tunnel for the hostname, routes its DNS record, and switches the
/// settings to the fixed tunnel. The DNS zone must be on the logged-in Cloudflare account.
pub async fn setup(store: &Store, hostname: &str) -> AppResult<Value> {
    if store.live.lock().unwrap().server.is_some() {
        return Err("请先停止连接，再配置固定隧道".into());
    }
    if !logged_in(store) {
        return Err("请先登录 Cloudflare".into());
    }
    let host = normalize_hostname(hostname)?;
    let name = tunnel_name(&host);
    let cert = cert_path(store);
    let cert = cert.to_string_lossy();
    let creds = credentials_path(store, &name);
    private_dir(creds.parent().unwrap())?;
    let creds_arg = creds.to_string_lossy().into_owned();

    if managed_tunnel_id(store, &name).is_none() {
        let created = run(
            store,
            &[
                "tunnel",
                "--origincert",
                &cert,
                "create",
                "--credentials-file",
                &creds_arg,
                &name,
            ],
        )
        .await;
        if let Err(e) = created {
            if !e.contains("already exists") {
                return Err(format!("创建隧道失败：{e}"));
            }
            // The tunnel exists on the account but its credentials are missing here; fetch them.
            run(
                store,
                &[
                    "tunnel",
                    "--origincert",
                    &cert,
                    "token",
                    "--credentials-file",
                    &creds_arg,
                    &name,
                ],
            )
            .await
            .map_err(|e| format!("读取已有隧道凭据失败：{e}"))?;
        }
    }
    let id = managed_tunnel_id(store, &name).ok_or("隧道凭据文件无效，请重试")?;
    run(
        store,
        &[
            "tunnel",
            "--origincert",
            &cert,
            "route",
            "dns",
            "--overwrite-dns",
            &id,
            &host,
        ],
    )
    .await
    .map_err(|e| format!("配置 DNS 失败（域名需托管在当前登录的 Cloudflare 账号下）：{e}"))?;

    let mut settings = store.config();
    settings.tunnel_mode = "named".into();
    settings.tunnel_name = name.clone();
    settings.public_url = format!("https://{host}");
    store.save_settings(settings)?;
    store.audit("cloudflare.setup", &host, true);
    Ok(json!({ "hostname": host, "tunnel_name": name, "tunnel_id": id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostnames_are_normalized_and_validated() {
        assert_eq!(
            normalize_hostname(" https://MCP.Example.com/mcp ").unwrap(),
            "mcp.example.com"
        );
        assert_eq!(
            normalize_hostname("a-b.example.co.uk").unwrap(),
            "a-b.example.co.uk"
        );
        for bad in [
            "",
            "localhost",
            "-a.example.com",
            "a..com",
            "a b.com",
            "a_b.com",
        ] {
            assert!(normalize_hostname(bad).is_err(), "{bad}");
        }
        assert_eq!(tunnel_name("mcp.example.com"), "cloudmcp-mcp-example-com");
    }

    #[test]
    fn managed_tunnel_reads_the_id_from_its_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("data")).unwrap();
        assert!(!logged_in(&store));
        assert_eq!(managed_tunnel_id(&store, "cloudmcp-a-com"), None);
        let creds = credentials_path(&store, "cloudmcp-a-com");
        std::fs::create_dir_all(creds.parent().unwrap()).unwrap();
        std::fs::write(
            &creds,
            r#"{"AccountTag":"x","TunnelSecret":"y","TunnelID":"0b1c"}"#,
        )
        .unwrap();
        assert_eq!(
            managed_tunnel_id(&store, "cloudmcp-a-com").as_deref(),
            Some("0b1c")
        );
    }
}
