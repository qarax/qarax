use std::net::SocketAddr;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax_node::rpc::node::vm_service_client::VmServiceClient;
use qarax_node::rpc::node::{Drive, VmConfig, VmId};
use tokio::net::TcpListener;
use tokio::time;

static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

async fn spawn_server() -> SocketAddr {
    Lazy::force(&TRACING);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = qarax_node::startup::run()
        .await
        .unwrap()
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));
    let _ = tokio::spawn(server);
    addr
}

async fn download(url: &str, path: &str) {
    let resp = reqwest::get(url).await.unwrap();
    let bytes = resp.bytes().await.unwrap();
    tokio::fs::write(path, bytes).await.unwrap();
}

#[tokio::test]
async fn test_start_vm() {
    const KERNEL_PATH: &str = "/var/tmp/vmlinux.bin";
    const KERNEL_URL: &str =
        "https://s3.amazonaws.com/spec.ccfc.min/img/hello/kernel/hello-vmlinux.bin";
    const ROOTFS_PATH: &str = "/var/tmp/hello-rootfs.ext4";
    const ROOTFS_URL: &str =
        "https://s3.amazonaws.com/spec.ccfc.min/img/hello/fsfiles/hello-rootfs.ext4";

    // Download kernel if it doesn't exist
    if !std::path::Path::new(KERNEL_PATH).exists() {
        download(KERNEL_URL, KERNEL_PATH).await;
    }

    // Download rootfs if it doesn't exist
    if !std::path::Path::new(ROOTFS_PATH).exists() {
        download(ROOTFS_URL, ROOTFS_PATH).await;
    }

    // Start qarax-node gRPC server
    let addr = spawn_server().await;

    // Create a gRPC client
    let mut client = VmServiceClient::connect(format!("http://{}", addr))
        .await
        .unwrap();

    let vm_id = uuid::Uuid::new_v4().to_string();
    // Create a request
    let request = tonic::Request::new(VmConfig {
        vm_id: vm_id.clone(),
        memory: 1024,
        vcpus: 1,
        kernel: KERNEL_PATH.to_string(),
        kernel_params: "console=ttyS0 reboot=k panic=1 pci=off random.trust_cpu=on".to_string(),
        drives: vec![Drive {
            id: "rootfs".to_string(),
            path_on_host: ROOTFS_PATH.to_string(),
            read_only: false,
            is_root: true,
            partuuid: None,
            io_engine: None,
        }],
    });

    // Call the RPC
    let r = client.start_vm(request).await;
    assert!(r.is_ok());

    let id_request = tonic::Request::new(VmId { id: vm_id.clone() });

    // Sleep for 2 seconds
    time::sleep(time::Duration::from_secs(2)).await;
    let result = client.get_vm_info(id_request).await.unwrap();
    assert_eq!(result.into_inner().status, 1);

    let id_request = tonic::Request::new(VmId { id: vm_id.clone() });
    // Stop the VM
    let result = client.stop_vm(id_request).await;
    assert!(result.is_ok());

    let id_request = tonic::Request::new(VmId { id: vm_id.clone() });
    let result = client.get_vm_info(id_request).await.unwrap();
    assert_eq!(result.into_inner().status, 2);
}
