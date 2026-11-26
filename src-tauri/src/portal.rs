use crate::store::AppResult;
use cap_std::{ambient_authority, fs::Dir};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::ErrorKind,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

const ENDPOINT_PATH: &str = "com.codex.portal/mcp-endpoint.json";
const UNAVAILABLE: &str = "Codex Portal 未运行或未开启 MCP 端点";
const READ_ENDPOINT_FAILED: &str = "无法读取 Codex Portal MCP 端点";
const INVALID_RESPONSE: &str = "Codex Portal 响应格式无效";

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Args {
    pub(crate) action: Action,
}

#[derive(Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    Status,
    Switch,
}

impl Action {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Switch => "switch",
        }
    }

    fn read_timeout(self) -> Duration {
        Duration::from_secs(match self {
            Self::Status => 15,
            Self::Switch => 180,
        })
    }
}

#[derive(Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
}

fn read_endpoint(data_dir: &Path) -> AppResult<Endpoint> {
    let bytes = Dir::open_ambient_dir(data_dir, ambient_authority())
        .and_then(|dir| dir.read(ENDPOINT_PATH))
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound => UNAVAILABLE,
            _ => READ_ENDPOINT_FAILED,
        })?;
    serde_json::from_slice(&bytes).map_err(|_| "Codex Portal MCP 端点格式无效".into())
}

pub(crate) async fn call(action: Action) -> AppResult<Value> {
    let data_dir = dirs::data_dir().ok_or(UNAVAILABLE)?;
    call_at(data_dir, action).await
}

async fn call_at(data_dir: PathBuf, action: Action) -> AppResult<Value> {
    let endpoint = tokio::task::spawn_blocking(move || read_endpoint(&data_dir))
        .await
        .map_err(|_| READ_ENDPOINT_FAILED)??;

    let mut stream = timeout(
        Duration::from_secs(3),
        TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)),
    )
    .await
    .map_err(|_| UNAVAILABLE)?
    .map_err(|_| UNAVAILABLE)?;

    let request = format!(
        "{}\n",
        json!({"token": endpoint.token, "command": action.as_str()})
    );
    timeout(Duration::from_secs(3), stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| "发送 Codex Portal 请求超时")?
        .map_err(|_| "发送 Codex Portal 请求失败")?;

    let mut response = String::new();
    timeout(
        action.read_timeout(),
        BufReader::new(stream).read_line(&mut response),
    )
    .await
    .map_err(|_| "等待 Codex Portal 响应超时")?
    .map_err(|_| "读取 Codex Portal 响应失败")?;
    if !response.ends_with('\n') {
        return Err(INVALID_RESPONSE.into());
    }

    let mut value =
        serde_json::from_str::<AppResult<Value>>(&response).map_err(|_| INVALID_RESPONSE)??;
    let fields = value.as_object_mut().ok_or(INVALID_RESPONSE)?;
    let all_exhausted = fields.get("allExhausted").and_then(Value::as_bool) == Some(true);
    fields.insert("retry_switch".into(), Value::Bool(!all_exhausted));
    if all_exhausted {
        let retry_after = fields
            .get("earliestRecoveryAt")
            .cloned()
            .unwrap_or(Value::Null);
        fields.insert("retry_after".into(), retry_after);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tokio::{
        net::{TcpListener, TcpSocket},
        task::JoinHandle,
    };

    const TEST_TOKEN: &str = "portal-test-token";

    fn write_endpoint(data_dir: &Path, port: u16) {
        let path = data_dir.join(ENDPOINT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json!({"port": port, "token": TEST_TOKEN}).to_string()).unwrap();
    }

    async fn fake_portal(
        data_dir: &Path,
        command: &'static str,
        response: AppResult<Value>,
    ) -> JoinHandle<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        write_endpoint(data_dir, listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream)
                .read_line(&mut line)
                .await
                .unwrap();
            assert!(line.ends_with('\n'));
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap(),
                json!({"token": TEST_TOKEN, "command": command}),
            );
            let line = format!("{}\n", serde_json::to_string(&response).unwrap());
            stream.write_all(line.as_bytes()).await.unwrap();
        })
    }

    #[tokio::test]
    async fn status_preserves_portal_fields_and_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let status = json!({
            "current": {"name": "current"},
            "next": null,
            "allExhausted": false,
            "earliestRecoveryAt": null,
        });
        let server = fake_portal(dir.path(), "status", Ok(status.clone())).await;
        let mut expected = status;
        expected["retry_switch"] = json!(true);
        assert_eq!(
            call_at(dir.path().into(), Action::Status).await.unwrap(),
            expected
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn exhausted_switch_returns_recovery_time() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({
            "switched": false,
            "current": null,
            "next": null,
            "allExhausted": true,
            "earliestRecoveryAt": "2026-11-20T18:00:00+08:00",
        });
        let server = fake_portal(dir.path(), "switch", Ok(result.clone())).await;
        let mut expected = result;
        expected["retry_switch"] = json!(false);
        expected["retry_after"] = expected["earliestRecoveryAt"].clone();
        assert_eq!(
            call_at(dir.path().into(), Action::Switch).await.unwrap(),
            expected
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn returns_portal_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let message = "智能切换正在执行或等待重试，请稍候。";
        let server = fake_portal(dir.path(), "switch", Err(message.into())).await;
        assert_eq!(
            call_at(dir.path().into(), Action::Switch)
                .await
                .unwrap_err(),
            message
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reports_missing_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            call_at(dir.path().into(), Action::Status)
                .await
                .unwrap_err(),
            UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn reports_connection_failure() {
        let dir = tempfile::tempdir().unwrap();
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        write_endpoint(dir.path(), socket.local_addr().unwrap().port());
        assert_eq!(
            call_at(dir.path().into(), Action::Status)
                .await
                .unwrap_err(),
            UNAVAILABLE
        );
    }

    #[test]
    fn rejects_invalid_action() {
        assert!(serde_json::from_value::<Args>(json!({"action": "activate"})).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_endpoint_symlink_outside_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_endpoint(outside.path(), 1);
        let path = dir.path().join(ENDPOINT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path().join(ENDPOINT_PATH), path).unwrap();
        assert!(read_endpoint(dir.path()).is_err());
    }
}
