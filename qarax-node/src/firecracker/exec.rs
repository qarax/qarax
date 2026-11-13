use std::{path::Path, time::Duration};

use bytes::Bytes;
use firecracker_rust_sdk::{client::TokioIo, models::Vsock};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::Request;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use super::*;

const MAX_GUEST_AGENT_HANDSHAKE_BYTES: u64 = 4096;
const MAX_GUEST_AGENT_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;
const SANDBOX_EXEC_PORT: u32 = 7000;

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

        let body = serde_json::to_vec(vsock)
            .map_err(|e| VmmError::InvalidConfig(format!("invalid vsock config: {}", e)))?;
        let request = Request::builder()
            .method("PUT")
            .uri("http://localhost/vsock")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(BoxBody::new(Full::<Bytes>::new(Bytes::from(body))))
            .map_err(|e| VmmError::ProcessError(format!("failed to build vsock request: {}", e)))?;

        let response = sender.send_request(request).await.map_err(|e| {
            VmmError::ProcessError(format!(
                "failed to configure Firecracker vsock device: {}",
                e
            ))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .into_body()
                .collect()
                .await
                .map_err(|e| {
                    VmmError::ProcessError(format!(
                        "failed to read Firecracker vsock error response: {}",
                        e
                    ))
                })?
                .to_bytes();
            return Err(VmmError::ProcessError(format!(
                "Firecracker vsock configuration failed ({}): {}",
                status,
                String::from_utf8_lossy(&body)
            )));
        }

        Ok(())
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

        let (status, vsock_socket_path) = {
            let vms = self.vms.lock().await;
            let instance = vms
                .get(vm_id)
                .ok_or_else(|| VmmError::VmNotFound(vm_id.to_string()))?;

            (
                instance.status,
                Self::vsock_socket_path_from_config(&instance.proto_config.vsock),
            )
        };

        if status != VmStatus::Running {
            return Err(VmmError::ExecUnavailable(format!(
                "VM {} is not running",
                vm_id
            )));
        }

        let vsock_socket_path = vsock_socket_path.ok_or_else(|| {
            VmmError::ExecUnavailable(format!(
                "exec guest agent is not configured for VM {}",
                vm_id
            ))
        })?;

        let request_json = Self::build_guest_exec_request(command, timeout_secs)?;
        let timeout = Self::exec_timeout(timeout_secs);

        let response_bytes = tokio::time::timeout(timeout, async {
            let mut stream = tokio::net::UnixStream::connect(&vsock_socket_path)
                .await
                .map_err(|e| {
                    VmmError::ExecError(format!(
                        "failed to connect to guest-agent socket {}: {}",
                        vsock_socket_path.display(),
                        e
                    ))
                })?;

            stream
                .write_all(format!("CONNECT {}\n", SANDBOX_EXEC_PORT).as_bytes())
                .await
                .map_err(|e| {
                    VmmError::ExecError(format!(
                        "failed to open vsock stream to guest agent: {}",
                        e
                    ))
                })?;

            let mut handshake = Vec::new();
            {
                let mut reader = tokio::io::BufReader::new(&mut stream);
                let mut limited = (&mut reader).take(MAX_GUEST_AGENT_HANDSHAKE_BYTES + 1);
                limited
                    .read_until(b'\n', &mut handshake)
                    .await
                    .map_err(|e| {
                        VmmError::ExecError(format!("failed to read guest-agent handshake: {}", e))
                    })?;
            }

            if !handshake.ends_with(b"\n")
                || handshake.len() > MAX_GUEST_AGENT_HANDSHAKE_BYTES as usize
            {
                return Err(VmmError::ExecError(format!(
                    "guest-agent handshake exceeds {} bytes or is missing a newline terminator",
                    MAX_GUEST_AGENT_HANDSHAKE_BYTES
                )));
            }

            let handshake = String::from_utf8(handshake).map_err(|e| {
                VmmError::ExecError(format!("guest-agent handshake was not valid UTF-8: {}", e))
            })?;
            if !handshake.starts_with("OK") {
                return Err(VmmError::ExecError(format!(
                    "guest-agent connect failed: {}",
                    handshake.trim_end()
                )));
            }

            stream.write_all(&request_json).await.map_err(|e| {
                VmmError::ExecError(format!("failed to send exec request to guest agent: {}", e))
            })?;

            let mut response = Vec::new();
            stream
                .take(MAX_GUEST_AGENT_RESPONSE_BYTES + 1)
                .read_to_end(&mut response)
                .await
                .map_err(|e| {
                    VmmError::ExecError(format!("failed to read guest-agent response: {}", e))
                })?;
            if response.len() > MAX_GUEST_AGENT_RESPONSE_BYTES as usize {
                return Err(VmmError::ExecError(format!(
                    "guest-agent response exceeds {} bytes",
                    MAX_GUEST_AGENT_RESPONSE_BYTES
                )));
            }
            Ok::<_, VmmError>(response)
        })
        .await
        .map_err(|_| VmmError::ExecTimeout(timeout.as_secs()))??;

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
        let manager = FirecrackerManager::new(runtime_dir.path(), "/bin/true");

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
