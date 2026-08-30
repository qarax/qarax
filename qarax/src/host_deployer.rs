use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use russh::{
    ChannelMsg, Disconnect, client,
    keys::{self, PrivateKeyWithHashAlg},
};
use tokio::{
    net::TcpStream,
    time::{Instant, sleep, timeout},
};
use tracing::{debug, info, warn};

use crate::model::hosts::{DeployHostRequest, Host};

const SSH_CONNECT_TIMEOUT_SECONDS: u64 = 15;
const NODE_WAIT_TIMEOUT_SECONDS: u64 = 300;
const NODE_POLL_INTERVAL_SECONDS: u64 = 5;
const REBOOT_DETECT_TIMEOUT_SECONDS: u64 = 90;

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("invalid host gRPC port: {0}")]
    InvalidNodePort(i32),

    #[error("SSH user is required")]
    MissingSshUser,

    #[error("no SSH authentication method provided")]
    MissingSshAuthentication,

    #[error("SSH connection to {address}:{port} timed out")]
    SshConnectTimeout { address: String, port: u16 },

    #[error("SSH authentication failed for user {user}")]
    AuthenticationFailed { user: String },

    #[error("failed to run SSH operation: {0}")]
    Ssh(#[from] russh::Error),

    #[error("failed to process SSH key: {0}")]
    SshKey(#[from] keys::Error),

    #[error("invalid SSH private key path: {path}")]
    InvalidSshKeyPath { path: String },

    #[error("remote command failed (status: {status:?})\nstdout: {stdout}\nstderr: {stderr}")]
    RemoteCommandFailed {
        status: Option<u32>,
        stdout: String,
        stderr: String,
    },

    #[error("qarax-node did not become reachable at {address}:{port} before timeout")]
    NodeUnavailable { address: String, port: u16 },
}

struct HostKeyCheckingHandler {
    host: String,
    port: u16,
}

impl client::Handler for HostKeyCheckingHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match keys::check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // Match previous `StrictHostKeyChecking=accept-new` behavior.
                if let Err(error) =
                    keys::known_hosts::learn_known_hosts(&self.host, self.port, server_public_key)
                {
                    warn!(
                        host = %self.host,
                        port = self.port,
                        error = %error,
                        "Failed to write host key to known_hosts; accepting for this session"
                    );
                }
                Ok(true)
            }
            Err(keys::Error::KeyChanged { .. }) => Ok(false),
            Err(error) => {
                warn!(
                    host = %self.host,
                    port = self.port,
                    error = %error,
                    "Failed to validate known_hosts entry; accepting for this session"
                );
                Ok(true)
            }
        }
    }
}

pub async fn deploy_bootc_host(
    host: &Host,
    request: &DeployHostRequest,
) -> Result<(), DeployError> {
    let node_port =
        u16::try_from(host.port).map_err(|_| DeployError::InvalidNodePort(host.port))?;
    let script = build_bootc_script(request);
    let remote_command = format!("bash -lc {}", shell_single_quote(&script));

    run_ssh_command(host, request, &remote_command, request.reboot()).await?;

    if request.reboot() {
        wait_for_reboot_transition(&host.address, node_port).await;
    }

    wait_for_node(&host.address, node_port).await?;
    Ok(())
}

fn build_bootc_script(request: &DeployHostRequest) -> String {
    let mut script = String::from(
        "set -euo pipefail\n\
run_privileged() {\n\
  if [ \"$(id -u)\" -eq 0 ]; then\n\
    \"$@\"\n\
  else\n\
    sudo -n \"$@\"\n\
  fi\n\
}\n",
    );

    if request.install_bootc() {
        script.push_str(
            "if ! command -v bootc >/dev/null 2>&1; then\n\
  if command -v dnf >/dev/null 2>&1; then\n\
    run_privileged dnf install -y bootc\n\
  elif command -v apt-get >/dev/null 2>&1; then\n\
    run_privileged apt-get update\n\
    run_privileged apt-get install -y bootc\n\
  else\n\
    echo \"bootc is not installed and no supported package manager was found\" >&2\n\
    exit 1\n\
  fi\n\
fi\n",
        );
    }

    script.push_str(&format!(
        "run_privileged bootc switch {}\n",
        shell_single_quote(request.image.trim())
    ));

    if request.reboot() {
        script.push_str("run_privileged systemctl reboot\n");
    }

    script
}

async fn run_ssh_command(
    host: &Host,
    request: &DeployHostRequest,
    remote_command: &str,
    allow_disconnect: bool,
) -> Result<(), DeployError> {
    let ssh_user = request
        .ssh_user
        .as_ref()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| host.host_user.trim().to_string());

    if ssh_user.is_empty() {
        return Err(DeployError::MissingSshUser);
    }

    let password = resolve_password(request);
    let mut session = connect_and_authenticate(host, request, &ssh_user, password).await?;
    let command_result =
        execute_remote_command(&mut session, remote_command, allow_disconnect).await;

    if let Err(error) = session
        .disconnect(Disconnect::ByApplication, "deploy complete", "en")
        .await
    {
        debug!(error = %error, "Failed to gracefully close SSH session");
    }

    command_result
}

async fn connect_and_authenticate(
    host: &Host,
    request: &DeployHostRequest,
    ssh_user: &str,
    password: Option<String>,
) -> Result<client::Handle<HostKeyCheckingHandler>, DeployError> {
    let ssh_port = request.ssh_port();
    let config = Arc::new(client::Config::default());
    let handler = HostKeyCheckingHandler {
        host: host.address.clone(),
        port: ssh_port,
    };

    let connect_result = timeout(
        Duration::from_secs(SSH_CONNECT_TIMEOUT_SECONDS),
        client::connect(config, (host.address.as_str(), ssh_port), handler),
    )
    .await;

    let mut session = match connect_result {
        Ok(Ok(session)) => session,
        Ok(Err(error)) => return Err(DeployError::Ssh(error)),
        Err(_) => {
            return Err(DeployError::SshConnectTimeout {
                address: host.address.clone(),
                port: ssh_port,
            });
        }
    };

    let auth_result = if let Some(password) = password {
        session
            .authenticate_password(ssh_user.to_string(), password)
            .await?
    } else if let Some(key_path) = request.ssh_private_key_path.as_deref() {
        let key_path = expand_private_key_path(key_path)?;
        let key_pair = keys::load_secret_key(&key_path, None)?;
        let key = PrivateKeyWithHashAlg::new(
            Arc::new(key_pair),
            session.best_supported_rsa_hash().await?.flatten(),
        );
        session
            .authenticate_publickey(ssh_user.to_string(), key)
            .await?
    } else {
        return Err(DeployError::MissingSshAuthentication);
    };

    if !auth_result.success() {
        return Err(DeployError::AuthenticationFailed {
            user: ssh_user.to_string(),
        });
    }

    Ok(session)
}

async fn execute_remote_command(
    session: &mut client::Handle<HostKeyCheckingHandler>,
    remote_command: &str,
    allow_disconnect: bool,
) -> Result<(), DeployError> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, remote_command).await?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status = None;

    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => stdout.extend_from_slice(data.as_ref()),
            ChannelMsg::ExtendedData { data, ext } => {
                if ext == 1 {
                    stderr.extend_from_slice(data.as_ref());
                } else {
                    stdout.extend_from_slice(data.as_ref());
                }
            }
            ChannelMsg::ExitStatus {
                exit_status: status,
            } => exit_status = Some(status),
            _ => {}
        }
    }

    match exit_status {
        Some(0) => Ok(()),
        Some(status) => Err(DeployError::RemoteCommandFailed {
            status: Some(status),
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        }),
        None if allow_disconnect => Ok(()),
        None => Err(DeployError::RemoteCommandFailed {
            status: None,
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        }),
    }
}

fn resolve_password(request: &DeployHostRequest) -> Option<String> {
    let request_password = request
        .ssh_password
        .as_ref()
        .map(|password| password.trim().to_string())
        .filter(|password| !password.is_empty());
    if request_password.is_some() {
        return request_password;
    }

    if request.ssh_private_key_path.is_some() {
        return None;
    }

    None
}

fn expand_private_key_path(path: &str) -> Result<PathBuf, DeployError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(DeployError::InvalidSshKeyPath {
            path: path.to_string(),
        });
    }

    if trimmed == "~" {
        let home = std::env::var("HOME").map_err(|_| DeployError::InvalidSshKeyPath {
            path: path.to_string(),
        })?;
        return Ok(PathBuf::from(home));
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var("HOME").map_err(|_| DeployError::InvalidSshKeyPath {
            path: path.to_string(),
        })?;
        return Ok(Path::new(&home).join(rest));
    }

    Ok(PathBuf::from(trimmed))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn wait_for_reboot_transition(address: &str, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(REBOOT_DETECT_TIMEOUT_SECONDS);
    while Instant::now() < deadline {
        if !is_port_open(address, port).await {
            info!(address = %address, port = port, "Detected host reboot transition");
            return;
        }
        sleep(Duration::from_secs(2)).await;
    }

    warn!(
        address = %address,
        port = port,
        "Did not observe host become unreachable after reboot; continuing with readiness checks"
    );
}

async fn wait_for_node(address: &str, port: u16) -> Result<(), DeployError> {
    let deadline = Instant::now() + Duration::from_secs(NODE_WAIT_TIMEOUT_SECONDS);
    while Instant::now() < deadline {
        if is_port_open(address, port).await {
            info!(address = %address, port = port, "qarax-node became reachable");
            return Ok(());
        }
        sleep(Duration::from_secs(NODE_POLL_INTERVAL_SECONDS)).await;
    }

    Err(DeployError::NodeUnavailable {
        address: address.to_string(),
        port,
    })
}

async fn is_port_open(address: &str, port: u16) -> bool {
    matches!(
        timeout(Duration::from_secs(3), TcpStream::connect((address, port))).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_single_quote_escapes_embedded_quotes() {
        assert_eq!(shell_single_quote("foo'bar"), "'foo'\"'\"'bar'");
    }

    #[test]
    fn bootc_script_respects_install_and_reboot_flags() {
        let request = DeployHostRequest {
            image: "quay.io/example/qarax-vmm:v1".to_string(),
            ssh_port: None,
            ssh_user: None,
            ssh_password: None,
            ssh_private_key_path: None,
            install_bootc: Some(false),
            reboot: Some(false),
        };

        let script = build_bootc_script(&request);
        assert!(!script.contains("dnf install -y bootc"));
        assert!(!script.contains("systemctl reboot"));
        assert!(script.contains("bootc switch 'quay.io/example/qarax-vmm:v1'"));
    }

    #[test]
    fn bootc_script_installs_bootc_when_requested() {
        let request = DeployHostRequest {
            image: "quay.io/example/qarax-vmm:v1".to_string(),
            ssh_port: None,
            ssh_user: None,
            ssh_password: None,
            ssh_private_key_path: None,
            install_bootc: Some(true),
            reboot: Some(true),
        };

        let script = build_bootc_script(&request);
        assert!(script.contains("dnf install -y bootc"));
        assert!(script.contains("apt-get install -y bootc"));
        assert!(script.contains("systemctl reboot"));
    }
}
