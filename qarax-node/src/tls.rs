//! mTLS for the node's gRPC API and NBD server. The control plane is the gRPC
//! API's only client; the NBD server's are other nodes, copying a backup from
//! it. Each must present a certificate issued by the CA in the node's TLS
//! directory, from which the node has a client certificate of its own too.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

/// The CA whose certificates clients must present.
const CA_CERT: &str = "ca-cert.pem";
/// This node's certificate, valid for the address it is registered under.
const SERVER_CERT: &str = "server-cert.pem";
const SERVER_KEY: &str = "server-key.pem";
/// This node's client certificate, which it presents to other nodes' NBD
/// servers.
const CLIENT_CERT: &str = "client-cert.pem";
const CLIENT_KEY: &str = "client-key.pem";

/// The gRPC server's TLS config from the files in `dir` (see
/// scripts/gen-node-tls.sh); fails if one is missing or invalid. Clients
/// without a certificate from its CA are refused during the handshake.
pub fn server_tls_config(dir: &Path) -> Result<ServerTlsConfig> {
    // tonic builds its rustls configs from the process-wide crypto provider,
    // and the dependency tree enables both of rustls' providers, so rustls
    // can't pick one itself. Err only means one is installed already.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read(&path).with_context(|| format!("failed to read {}", path.display()))
    };
    let config = ServerTlsConfig::new()
        .identity(Identity::from_pem(read(SERVER_CERT)?, read(SERVER_KEY)?))
        .client_ca_root(Certificate::from_pem(read(CA_CERT)?));
    // Parses the files, which tonic otherwise leaves until the server starts.
    tonic::transport::Server::builder()
        .tls_config(config.clone())
        .with_context(|| format!("invalid TLS files in {}", dir.display()))?;
    Ok(config)
}

fn certificates(dir: &Path, name: &str) -> Result<Vec<CertificateDer<'static>>> {
    let path = dir.join(name);
    CertificateDer::pem_file_iter(&path)
        .and_then(|certs| certs.collect())
        .with_context(|| format!("failed to read {}", path.display()))
}

fn private_key(dir: &Path, name: &str) -> Result<PrivateKeyDer<'static>> {
    let path = dir.join(name);
    PrivateKeyDer::from_pem_file(&path)
        .with_context(|| format!("failed to read {}", path.display()))
}

fn ca_roots(dir: &Path) -> Result<Arc<RootCertStore>> {
    let mut roots = RootCertStore::empty();
    for cert in certificates(dir, CA_CERT)? {
        roots.add(cert)?;
    }
    Ok(Arc::new(roots))
}

/// The NBD server's TLS config from the files in `dir`: the node's
/// certificate, and clients need one from its CA.
pub fn nbd_server_config(dir: &Path) -> Result<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier =
        WebPkiClientVerifier::builder_with_provider(ca_roots(dir)?, provider.clone()).build()?;
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            certificates(dir, SERVER_CERT)?,
            private_key(dir, SERVER_KEY)?,
        )
        .with_context(|| format!("invalid TLS files in {}", dir.display()))
}

/// TLS config from the files in `dir` for copying from another node's NBD
/// server: trusts the node CA, and presents this node's client certificate,
/// which `scripts/gen-node-tls.sh` issues to directories from before nodes had
/// one when run again.
pub fn nbd_client_config(dir: &Path) -> Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(ca_roots(dir)?)
        .with_client_auth_cert(
            certificates(dir, CLIENT_CERT)?,
            private_key(dir, CLIENT_KEY)?,
        )
        .with_context(|| format!("invalid TLS files in {}", dir.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::{Channel, ClientTlsConfig, Server};
    use uuid::Uuid;

    use super::*;
    use crate::rpc::node::{
        DeleteStorageFilesRequest, StoragePoolKind,
        file_transfer_service_client::FileTransferServiceClient,
        file_transfer_service_server::FileTransferServiceServer,
    };
    use crate::services::file_transfer::FileTransferServiceImpl;

    /// A new CA and its certificates, made with the script operators use.
    pub(crate) fn gen_node_tls(nodes: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("sh")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../scripts/gen-node-tls.sh"
            ))
            .arg(dir.path())
            .args(nodes)
            .status()
            .unwrap();
        assert!(status.success(), "gen-node-tls.sh failed");
        dir
    }

    /// The control plane's side of `certs`: trusts its CA, presents its
    /// client certificate.
    fn control_plane_tls(certs: &Path) -> ClientTlsConfig {
        let read = |name: &str| std::fs::read(certs.join("control-plane").join(name)).unwrap();
        ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(read("ca-cert.pem")))
            .identity(Identity::from_pem(
                read("client-cert.pem"),
                read("client-key.pem"),
            ))
    }

    /// Serves the file transfer service with the TLS config main gives the
    /// server, from the directory a node registered as 127.0.0.1 gets.
    async fn serve(certs: &Path) -> String {
        let tls = server_tls_config(&certs.join("nodes/127.0.0.1")).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("https://{}", listener.local_addr().unwrap());
        tokio::spawn(
            Server::builder()
                .tls_config(tls)
                .unwrap()
                .add_service(FileTransferServiceServer::new(
                    FileTransferServiceImpl::default(),
                ))
                .serve_with_incoming(TcpListenerStream::new(listener)),
        );
        address
    }

    /// Deletes a file that doesn't exist from a scratch Local pool, which
    /// succeeds whenever the request reaches the service.
    async fn delete_nothing(address: &str, tls: ClientTlsConfig) -> Result<(), String> {
        let root = tempfile::tempdir().unwrap();
        let channel = Channel::from_shared(address.to_string())
            .unwrap()
            .tls_config(tls)
            .unwrap()
            .connect()
            .await
            .map_err(|e| e.to_string())?;
        FileTransferServiceClient::new(channel)
            .delete_storage_files(DeleteStorageFilesRequest {
                pool_id: Uuid::new_v4().to_string(),
                object_id: Uuid::new_v4().to_string(),
                pool_kind: StoragePoolKind::Local as i32,
                config_json: serde_json::json!({ "path": root.path() }).to_string(),
                upper_layer: false,
            })
            .await
            .map(|_| ())
            .map_err(|status| status.to_string())
    }

    #[tokio::test]
    async fn serves_only_clients_with_a_certificate_from_its_ca() {
        let certs = gen_node_tls(&["127.0.0.1"]);
        let address = serve(certs.path()).await;

        delete_nothing(&address, control_plane_tls(certs.path()))
            .await
            .expect("the control plane's certificate is accepted");

        let ca = std::fs::read(certs.path().join("ca-cert.pem")).unwrap();
        let no_certificate = ClientTlsConfig::new().ca_certificate(Certificate::from_pem(&ca));
        delete_nothing(&address, no_certificate)
            .await
            .expect_err("a client without a certificate is refused");

        // A certificate from another CA, trusting this node's.
        let other = gen_node_tls(&[]);
        let read =
            |name: &str| std::fs::read(other.path().join("control-plane").join(name)).unwrap();
        let impostor = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(&ca))
            .identity(Identity::from_pem(
                read("client-cert.pem"),
                read("client-key.pem"),
            ));
        delete_nothing(&address, impostor)
            .await
            .expect_err("a client certificate from another CA is refused");

        let plaintext = Channel::from_shared(address.replace("https://", "http://"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        FileTransferServiceClient::new(plaintext)
            .delete_storage_files(DeleteStorageFilesRequest::default())
            .await
            .expect_err("a plaintext client is refused");
    }
}
