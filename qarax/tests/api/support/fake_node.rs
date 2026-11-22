//! A qarax-node that forks VMs and does nothing else, for what the control
//! plane does around a fork: it answers ForkVM and ForkSeed, keeping each
//! request, and refuses every other call.

use std::sync::{Arc, Mutex};

use qarax::grpc_client::node::{
    self,
    vm_service_server::{VmService, VmServiceServer},
};
use tokio::net::TcpListener;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::{TestApp, node_server_tls};

#[derive(Clone, Default)]
pub struct ForkingNode {
    forks: Arc<Mutex<Vec<node::ForkVmRequest>>>,
    seed_forks: Arc<Mutex<Vec<node::ForkVmRequest>>>,
}

impl ForkingNode {
    /// The ForkVM requests it has answered, oldest first.
    pub fn forks(&self) -> Vec<node::ForkVmRequest> {
        self.forks.lock().unwrap().clone()
    }

    /// The ForkSeed requests it has answered, oldest first.
    pub fn seed_forks(&self) -> Vec<node::ForkVmRequest> {
        self.seed_forks.lock().unwrap().clone()
    }
}

/// Serves a [`ForkingNode`] on 127.0.0.1, the address its certificate names,
/// and registers it as an UP host named `name`.
pub async fn add_forking_node(app: &TestApp, name: &str) -> (Uuid, ForkingNode) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let node = ForkingNode::default();
    let incoming = async_stream::stream! {
        loop { yield listener.accept().await.map(|(stream, _)| stream); }
    };
    tokio::spawn(
        tonic::transport::Server::builder()
            .tls_config(node_server_tls())
            .unwrap()
            .add_service(VmServiceServer::new(node.clone()))
            .serve_with_incoming(incoming),
    );
    let host_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO hosts (id, name, address, port, host_user, status) \
         VALUES ($1, $2, '127.0.0.1', $3, 'root', 'UP')",
    )
    .bind(host_id)
    .bind(name)
    .bind(i32::from(port))
    .execute(&app.pool)
    .await
    .unwrap();
    (host_id, node)
}

/// `impl VmService` with `body`, and an `unimplemented` answer for each
/// method listed after it.
macro_rules! vm_service {
    ($node:ty { $($body:tt)* } $($method:ident($request:ty) -> $response:ty;)*) => {
        #[tonic::async_trait]
        impl VmService for $node {
            $($body)*
            $(
                async fn $method(
                    &self,
                    _: Request<$request>,
                ) -> Result<Response<$response>, Status> {
                    Err(Status::unimplemented(stringify!($method)))
                }
            )*
        }
    };
}

vm_service! {
    ForkingNode {
        type AttachConsoleStream =
            futures::stream::Empty<Result<node::ConsoleOutput, Status>>;

        async fn fork_vm(
            &self,
            request: Request<node::ForkVmRequest>,
        ) -> Result<Response<()>, Status> {
            self.forks.lock().unwrap().push(request.into_inner());
            Ok(Response::new(()))
        }

        async fn fork_seed(
            &self,
            request: Request<node::ForkVmRequest>,
        ) -> Result<Response<()>, Status> {
            self.seed_forks.lock().unwrap().push(request.into_inner());
            Ok(Response::new(()))
        }
    }
    get_node_info(()) -> node::NodeInfo;
    create_vm(node::VmConfig) -> node::VmState;
    start_vm(node::VmId) -> ();
    stop_vm(node::VmId) -> ();
    force_stop_vm(node::VmId) -> ();
    pause_vm(node::VmId) -> ();
    resume_vm(node::VmId) -> ();
    snapshot_vm(node::SnapshotVmRequest) -> ();
    restore_vm(node::RestoreVmRequest) -> ();
    delete_snapshot(node::DeleteSnapshotRequest) -> ();
    delete_vm(node::VmId) -> ();
    receive_migration(node::ReceiveMigrationRequest) -> node::ReceiveMigrationResponse;
    send_migration(node::SendMigrationRequest) -> ();
    get_vm_info(node::VmId) -> node::VmState;
    list_vms(()) -> node::VmList;
    get_vm_counters(node::VmId) -> node::VmCounters;
    read_console_log(node::VmId) -> node::ConsoleLogResponse;
    exec_vm(node::ExecVmRequest) -> node::ExecVmResponse;
    wait_for_guest_agent(node::WaitForGuestAgentRequest) -> ();
    attach_console(tonic::Streaming<node::ConsoleInput>) -> Self::AttachConsoleStream;
    get_console_pty_path(node::VmId) -> node::ConsolePtyPathResponse;
    add_network_device(node::AddNetworkDeviceRequest) -> ();
    remove_network_device(node::RemoveDeviceRequest) -> ();
    add_disk_device(node::AddDiskDeviceRequest) -> ();
    remove_disk_device(node::RemoveDeviceRequest) -> ();
    add_device(node::AddDeviceRequest) -> ();
    remove_vfio_device(node::RemoveDeviceRequest) -> ();
    resize_vm(node::ResizeVmRequest) -> ();
    resize_disk(node::ResizeDiskRequest) -> ();
    probe_image_format(node::ProbeImageFormatRequest) -> node::ProbeImageFormatResponse;
    preflight_image(node::PreflightImageRequest) -> node::PreflightImageResponse;
    import_overlay_bd_image(node::ImportOverlayBdRequest) -> node::ImportOverlayBdResponse;
    attach_storage_pool(node::AttachStoragePoolRequest) -> node::AttachStoragePoolResponse;
    detach_storage_pool(node::DetachStoragePoolRequest) -> ();
    attach_network(node::AttachNetworkRequest) -> node::AttachNetworkResponse;
    detach_network(node::DetachNetworkRequest) -> node::DetachNetworkResponse;
    sync_network_isolation(node::SyncNetworkIsolationRequest) -> ();
    sync_vpc_overlays(node::SyncVpcOverlaysRequest) -> ();
    sync_vm_firewall(node::SyncVmFirewallRequest) -> ();
}
