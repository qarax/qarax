use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    time::Duration,
};

use bytes::Bytes;
use firecracker_rust_sdk::{
    client::TokioIo,
    models::{InstanceInfo, Vsock},
};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::Request;

use super::*;
use crate::vmm::VMM_ANSWER_TIMEOUT;

#[derive(serde::Deserialize)]
struct GuestExecResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

impl FirecrackerManager {
    fn exec_timeout(timeout_secs: Option<u64>) -> Duration {
        const GRACE_SECS: u64 = 5;
        const SAFETY_TIMEOUT_SECS: u64 = 300;
        Duration::from_secs(
            timeout_secs
                .map(|s| s + GRACE_SECS)
                .unwrap_or(SAFETY_TIMEOUT_SECS),
        )
    }

    pub(super) fn build_guest_exec_request(
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Vec<u8>, VmmError> {
        #[derive(serde::Serialize)]
        struct GuestExecRequest {
            command: Vec<String>,
            timeout_secs: Option<u64>,
        }

        let mut request_json = serde_json::to_vec(&GuestExecRequest {
            command,
            timeout_secs,
        })
        .map_err(|e| VmmError::ExecInvalid(e.to_string()))?;
        request_json.push(b'\n');
        Ok(request_json)
    }

    pub(super) async fn put_vsock_device(
        socket_path: &Path,
        vsock: &Vsock,
    ) -> Result<(), VmmError> {
        Self::api_request(socket_path, "PUT", "/vsock", vsock).await
    }

    /// One request to the Firecracker API at `socket_path`, for what the SDK
    /// has no call for.
    pub(super) async fn api_request(
        socket_path: &Path,
        method: &str,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<(), VmmError> {
        let body = serde_json::to_vec(body)
            .map_err(|e| VmmError::InvalidConfig(format!("invalid body for {path}: {e}")))?;
        let request = Request::builder()
            .method(method)
            .uri(format!("http://localhost{path}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(BoxBody::new(Full::<Bytes>::new(Bytes::from(body))))
            .map_err(|e| VmmError::ProcessError(format!("failed to build {method} {path}: {e}")))?;
        Self::send(socket_path, request).await?;
        Ok(())
    }

    /// What Firecracker reports of its instance, given up on if it doesn't
    /// answer within [`VMM_ANSWER_TIMEOUT`]. A request dropped unanswered
    /// closes the connection it went on, so this one has its own: the SDK
    /// handle's would fail every request after it.
    pub(super) async fn describe_instance(socket_path: &Path) -> Result<InstanceInfo, VmmError> {
        let request = Request::builder()
            .method("GET")
            .uri("http://localhost/")
            .header("Accept", "application/json")
            .body(BoxBody::new(Empty::<Bytes>::new()))
            .map_err(|e| VmmError::ProcessError(format!("failed to build GET /: {e}")))?;
        let Ok(body) =
            tokio::time::timeout(VMM_ANSWER_TIMEOUT, Self::send(socket_path, request)).await
        else {
            warn!(
                "Firecracker at {} did not answer GET / within {}s",
                socket_path.display(),
                VMM_ANSWER_TIMEOUT.as_secs()
            );
            return Err(VmmError::ProcessError(format!(
                "no answer to GET / within {}s",
                VMM_ANSWER_TIMEOUT.as_secs()
            )));
        };
        serde_json::from_slice(&body?)
            .map_err(|e| VmmError::ProcessError(format!("unexpected instance info: {e}")))
    }

    /// Send `request` to the Firecracker API at `socket_path` on a connection
    /// of its own, and read the body of its answer.
    async fn send(
        socket_path: &Path,
        request: Request<BoxBody<Bytes, Infallible>>,
    ) -> Result<Bytes, VmmError> {
        let (method, path) = (request.method().clone(), request.uri().path().to_string());
        let stream = tokio::net::UnixStream::connect(socket_path)
            .await
            .map_err(|e| {
                VmmError::ProcessError(format!(
                    "failed to connect to Firecracker API socket {}: {}",
                    socket_path.display(),
                    e
                ))
            })?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| {
                VmmError::ProcessError(format!(
                    "failed to establish Firecracker API connection {}: {}",
                    socket_path.display(),
                    e
                ))
            })?;
        tokio::spawn(conn);
        sender.ready().await.map_err(|e| {
            VmmError::ProcessError(format!(
                "Firecracker API connection was not ready {}: {}",
                socket_path.display(),
                e
            ))
        })?;

        let response = sender
            .send_request(request)
            .await
            .map_err(|e| VmmError::ProcessError(format!("Firecracker {method} {path}: {e}")))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|e| {
                VmmError::ProcessError(format!(
                    "failed to read Firecracker's answer to {method} {path}: {e}"
                ))
            })?
            .to_bytes();
        if !status.is_success() {
            return Err(VmmError::ProcessError(format!(
                "Firecracker {method} {path} failed ({}): {}",
                status,
                String::from_utf8_lossy(&body)
            )));
        }

        Ok(body)
    }

    /// The host end of `vm_id`'s guest-agent vsock, while the VM runs.
    pub(super) async fn guest_agent_socket(&self, vm_id: &str) -> Result<PathBuf, VmmError> {
        let instance = self.lock_vm(vm_id).await?;
        if instance.status() != VmStatus::Running {
            return Err(VmmError::ExecUnavailable(format!(
                "VM {} is not running",
                vm_id
            )));
        }
        Self::vsock_socket_path_from_config(&instance.proto_config.vsock).ok_or_else(|| {
            VmmError::ExecUnavailable(format!(
                "exec guest agent is not configured for VM {}",
                vm_id
            ))
        })
    }

    pub async fn exec_vm(
        &self,
        vm_id: &str,
        command: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<crate::rpc::node::ExecVmResponse, VmmError> {
        if command.is_empty() {
            return Err(VmmError::ExecInvalid(
                "command must contain at least one argument".into(),
            ));
        }

        let vsock_socket_path = self.guest_agent_socket(vm_id).await?;
        let request_json = Self::build_guest_exec_request(command, timeout_secs)?;
        let timeout = Self::exec_timeout(timeout_secs);
        let response_bytes =
            guest_agent::request(&vsock_socket_path, &request_json, timeout).await?;

        let response: GuestExecResponse = serde_json::from_slice(&response_bytes)
            .map_err(|e| VmmError::ExecError(format!("invalid guest-agent response: {}", e)))?;

        Ok(crate::rpc::node::ExecVmResponse {
            exit_code: response.exit_code,
            stdout: response.stdout,
            stderr: response.stderr,
            timed_out: response.timed_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[tokio::test]
    async fn exec_vm_rejects_empty_command() {
        let runtime_dir = tempfile::TempDir::new().unwrap();
        let manager = FirecrackerManager::new(
            runtime_dir.path(),
            "/bin/true",
            Duration::from_secs(1),
            None,
        );

        let err = manager
            .exec_vm("test-vm", Vec::new(), None)
            .await
            .unwrap_err();
        assert_matches!(err, VmmError::ExecInvalid(_));
    }

    #[test]
    fn build_guest_exec_request_is_newline_framed_json() {
        let payload = FirecrackerManager::build_guest_exec_request(
            vec!["/bin/echo".into(), "hello".into()],
            Some(5),
        )
        .unwrap();

        assert_eq!(payload.last(), Some(&b'\n'));

        let body = std::str::from_utf8(&payload[..payload.len() - 1]).unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["command"], serde_json::json!(["/bin/echo", "hello"]));
        assert_eq!(json["timeout_secs"], 5);
    }
}
