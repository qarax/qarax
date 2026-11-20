//! The node's end of the guest agent: qarax-init, listening on vsock port
//! 7000 in the guest. Both VMMs expose a guest's vsock as a Unix socket on the
//! host, where a client writes `CONNECT <port>` and the VMM answers
//! `OK <port>` once a guest process accepts; while nothing listens on the
//! port, it closes the connection instead. A connection carries one request
//! line and its answer.

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tracing::debug;

use super::{VmmError, VmmManager};

const PORT: u32 = 7000;
const MAX_HANDSHAKE_BYTES: u64 = 4096;
const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

/// Asks the agent whether it is up.
const PING: &[u8] = b"{\"ping\":{}}\n";

/// The agent's answer to a request other than exec: an exec response, whose
/// exit code is 0 when it accepted, with why it didn't on stderr.
#[derive(serde::Deserialize)]
pub(crate) struct Ack {
    pub(crate) exit_code: i32,
    #[serde(default)]
    pub(crate) stderr: String,
}

/// Send one request line to the guest agent behind `vsock_socket_path`
/// and read its whole response, within `timeout`.
pub(crate) async fn request(
    vsock_socket_path: &Path,
    request_json: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, VmmError> {
    tokio::time::timeout(timeout, async {
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
            .write_all(format!("CONNECT {}\n", PORT).as_bytes())
            .await
            .map_err(|e| {
                VmmError::ExecError(format!("failed to open vsock stream to guest agent: {}", e))
            })?;

        let mut handshake = Vec::new();
        {
            let mut reader = tokio::io::BufReader::new(&mut stream);
            let mut limited = (&mut reader).take(MAX_HANDSHAKE_BYTES + 1);
            limited
                .read_until(b'\n', &mut handshake)
                .await
                .map_err(|e| {
                    VmmError::ExecError(format!("failed to read guest-agent handshake: {}", e))
                })?;
        }

        if !handshake.ends_with(b"\n") || handshake.len() > MAX_HANDSHAKE_BYTES as usize {
            return Err(VmmError::ExecError(format!(
                "guest-agent handshake exceeds {} bytes or is missing a newline terminator",
                MAX_HANDSHAKE_BYTES
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

        stream.write_all(request_json).await.map_err(|e| {
            VmmError::ExecError(format!("failed to send exec request to guest agent: {}", e))
        })?;

        let mut response = Vec::new();
        stream
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut response)
            .await
            .map_err(|e| {
                VmmError::ExecError(format!("failed to read guest-agent response: {}", e))
            })?;
        if response.len() > MAX_RESPONSE_BYTES as usize {
            return Err(VmmError::ExecError(format!(
                "guest-agent response exceeds {} bytes",
                MAX_RESPONSE_BYTES
            )));
        }
        Ok::<_, VmmError>(response)
    })
    .await
    .map_err(|_| VmmError::ExecTimeout(timeout.as_secs()))?
}

/// Wait until `vm_id`'s guest agent answers, at most `timeout`. Fails as
/// soon as the VM stops running.
pub(crate) async fn wait_until_up(
    manager: &dyn VmmManager,
    vm_id: &str,
    timeout: Duration,
) -> Result<(), VmmError> {
    // Until the agent listens, the VMM refuses a connection at once.
    const RETRY_INTERVAL: Duration = Duration::from_millis(50);

    let deadline = Instant::now() + timeout;
    loop {
        let socket = manager.guest_agent_socket(vm_id).await?;
        match ping(&socket).await {
            Ok(()) => return Ok(()),
            Err(e) if Instant::now() < deadline => {
                debug!("Guest agent of VM {vm_id} does not answer yet: {e}");
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
            Err(e) => {
                debug!("Guest agent of VM {vm_id} did not answer: {e}");
                return Err(VmmError::ExecTimeout(timeout.as_secs()));
            }
        }
    }
}

/// Whether the agent behind `socket` answers. One older than the ping
/// answers it as an invalid exec request: an answer all the same.
async fn ping(socket: &Path) -> Result<(), VmmError> {
    const TIMEOUT: Duration = Duration::from_secs(2);

    let response = request(socket, PING, TIMEOUT).await?;
    serde_json::from_slice(&response)
        .map(|_: Ack| ())
        .map_err(|e| VmmError::ExecError(format!("invalid guest-agent response: {e}")))
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;
    use tokio::net::UnixListener;

    use super::*;

    /// A VMM's vsock socket with the agent listening behind it: it answers
    /// one connection's request with `answer`, and returns the request.
    fn agent(socket: &Path, answer: &'static str) -> tokio::task::JoinHandle<String> {
        let listener = UnixListener::bind(socket).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut connect = String::new();
            stream.read_line(&mut connect).await.unwrap();
            assert_eq!(connect, "CONNECT 7000\n");
            stream.get_mut().write_all(b"OK 7000\n").await.unwrap();
            let mut request = String::new();
            stream.read_line(&mut request).await.unwrap();
            stream.get_mut().write_all(answer.as_bytes()).await.unwrap();
            request
        })
    }

    #[tokio::test]
    async fn an_agent_answers_a_ping_whatever_its_version() {
        let current = r#"{"exit_code":0,"stdout":"","stderr":"","timed_out":false}"#;
        let older = r#"{"exit_code":2,"stdout":"","stderr":"invalid exec request: missing field `command`","timed_out":false}"#;
        for answer in [current, older] {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("vsock");
            let served = agent(&socket, answer);
            ping(&socket).await.unwrap();
            assert_eq!(served.await.unwrap().as_bytes(), PING);
        }
    }

    #[tokio::test]
    async fn a_closed_connection_is_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("vsock");
        let listener = UnixListener::bind(&socket).unwrap();
        // What the VMM does while nothing in the guest listens on the port.
        let vmm = tokio::spawn(async move { drop(listener.accept().await.unwrap()) });
        assert!(ping(&socket).await.is_err());
        vmm.await.unwrap();
    }
}
