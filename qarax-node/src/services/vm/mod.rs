use anyhow::Result as AnyhowResult;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, instrument, warn};

use crate::cloud_hypervisor::VmManager;
use crate::rpc::node::{
    AddDeviceRequest, AddDiskDeviceRequest, AddFsDeviceRequest, AddNetworkDeviceRequest,
    AttachNetworkRequest, AttachNetworkResponse, AttachStoragePoolRequest,
    AttachStoragePoolResponse, ConsoleInput, ConsoleLogResponse, ConsoleOutput,
    ConsolePtyPathResponse, DetachNetworkRequest, DetachNetworkResponse, DetachStoragePoolRequest,
    DeviceCounters, GpuInfo, ImportOverlayBdRequest, ImportOverlayBdResponse, NodeInfo,
    OciImageRequest, OciImageResponse, ReceiveMigrationRequest, ReceiveMigrationResponse,
    RemoveDeviceRequest, ResizeVmRequest, RestoreVmRequest, SendMigrationRequest,
    SnapshotVmRequest, StoragePoolKind, VmConfig, VmCounters, VmId, VmList, VmState,
    vm_service_server::VmService,
};

/// Implementation of VmService using Cloud Hypervisor
#[derive(Clone)]
pub struct VmServiceImpl {
    manager: Arc<VmManager>,
}

impl VmServiceImpl {
    /// Create a new VmServiceImpl with default paths (no Nydus support)
    pub async fn new() -> Self {
        Self::with_paths("/var/lib/qarax/vms", "/usr/local/bin/cloud-hypervisor").await
    }

    /// Create a new VmServiceImpl with custom paths (no Nydus support)
    pub async fn with_paths(
        runtime_dir: impl Into<std::path::PathBuf>,
        ch_binary: impl Into<std::path::PathBuf>,
    ) -> Self {
        let manager = Arc::new(VmManager::new(runtime_dir, ch_binary, None));
        manager.recover_vms().await;
        Self { manager }
    }

    /// Create from an existing VmManager
    pub fn from_manager(manager: Arc<VmManager>) -> Self {
        Self { manager }
    }

    /// Run a unit-returning VM operation, logging start/success/failure uniformly.
    async fn run_vm_op<F, Fut>(
        &self,
        op_name: &str,
        vm_id: String,
        f: F,
    ) -> Result<Response<()>, Status>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), crate::cloud_hypervisor::VmManagerError>>,
    {
        info!("{} VM: {}", op_name, vm_id);
        match f().await {
            Ok(()) => {
                info!("VM {} {}d successfully", vm_id, op_name.to_lowercase());
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to {} VM {}: {}", op_name.to_lowercase(), vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }
}

#[tonic::async_trait]
impl VmService for VmServiceImpl {
    async fn create_vm(&self, request: Request<VmConfig>) -> Result<Response<VmState>, Status> {
        let config = request.into_inner();
        let vm_id = config.vm_id.clone();

        info!("Creating VM: {}", vm_id);
        debug!("VM config: {:?}", config);

        match self.manager.create_vm(config).await {
            Ok(state) => {
                info!("VM {} created successfully", vm_id);
                Ok(Response::new(state))
            }
            Err(e) => {
                error!("Failed to create VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn start_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("Start", vm_id.clone(), || self.manager.start_vm(&vm_id))
            .await
    }

    async fn stop_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("Stop", vm_id.clone(), || self.manager.stop_vm(&vm_id))
            .await
    }

    async fn force_stop_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("ForceStop", vm_id.clone(), || {
            self.manager.force_stop_vm(&vm_id)
        })
        .await
    }

    async fn pause_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("Pause", vm_id.clone(), || self.manager.pause_vm(&vm_id))
            .await
    }

    async fn resume_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("Resume", vm_id.clone(), || self.manager.resume_vm(&vm_id))
            .await
    }

    async fn snapshot_vm(
        &self,
        request: Request<SnapshotVmRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Snapshotting VM: {}", req.vm_id);
        match self
            .manager
            .snapshot_vm(&req.vm_id, &req.snapshot_url)
            .await
        {
            Ok(()) => {
                info!("VM {} snapshotted successfully", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to snapshot VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn restore_vm(&self, request: Request<RestoreVmRequest>) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Restoring VM: {}", req.vm_id);
        match self.manager.restore_vm(&req.vm_id, &req.source_url).await {
            Ok(()) => {
                info!("VM {} restored successfully", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to restore VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn delete_vm(&self, request: Request<VmId>) -> Result<Response<()>, Status> {
        let vm_id = request.into_inner().id;
        self.run_vm_op("Delete", vm_id.clone(), || self.manager.delete_vm(&vm_id))
            .await
    }

    async fn get_vm_info(&self, request: Request<VmId>) -> Result<Response<VmState>, Status> {
        let vm_id = request.into_inner().id;
        info!("Getting VM info: {}", vm_id);

        match self.manager.get_vm_info(&vm_id).await {
            Ok(state) => Ok(Response::new(state)),
            Err(e) => {
                error!("Failed to get VM info {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn list_vms(&self, _request: Request<()>) -> Result<Response<VmList>, Status> {
        info!("Listing VMs");

        let vms = self.manager.list_vms().await;
        info!("Found {} VMs", vms.len());
        Ok(Response::new(VmList { vms }))
    }

    async fn get_vm_counters(
        &self,
        request: Request<VmId>,
    ) -> Result<Response<VmCounters>, Status> {
        let vm_id = request.into_inner().id;
        info!("Getting VM counters: {}", vm_id);

        match self.manager.get_vm_counters(&vm_id).await {
            Ok(counters) => {
                let proto_counters = counters
                    .into_iter()
                    .map(|(device, values)| (device, DeviceCounters { values }))
                    .collect();
                Ok(Response::new(VmCounters {
                    counters: proto_counters,
                }))
            }
            Err(e) => {
                error!("Failed to get VM counters {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn add_network_device(
        &self,
        request: Request<AddNetworkDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding network device to VM: {}", req.vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing network config"))?;

        match self.manager.add_network_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("Network device added to VM {}", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to add network device to VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_network_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing network device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self
            .manager
            .remove_network_device(&req.vm_id, &req.device_id)
            .await
        {
            Ok(()) => {
                info!(
                    "Network device {} removed from VM {}",
                    req.device_id, req.vm_id
                );
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove network device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn add_disk_device(
        &self,
        request: Request<AddDiskDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding disk device to VM: {}", req.vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing disk config"))?;

        match self.manager.add_disk_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("Disk device added to VM {}", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to add disk device to VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_disk_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing disk device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self
            .manager
            .remove_disk_device(&req.vm_id, &req.device_id)
            .await
        {
            Ok(()) => {
                info!(
                    "Disk device {} removed from VM {}",
                    req.device_id, req.vm_id
                );
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove disk device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn add_fs_device(
        &self,
        request: Request<AddFsDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!("Adding fs device to VM: {}", req.vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing fs config"))?;

        match self.manager.add_fs_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("Fs device added to VM {}", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to add fs device to VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_fs_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing fs device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self
            .manager
            .remove_fs_device(&req.vm_id, &req.device_id)
            .await
        {
            Ok(()) => {
                info!("Fs device {} removed from VM {}", req.device_id, req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove fs device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn add_device(&self, request: Request<AddDeviceRequest>) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("config is required"))?;
        info!("Adding VFIO device {} to VM: {}", config.id, req.vm_id);

        match self.manager.add_device(&req.vm_id, &config).await {
            Ok(()) => {
                info!("VFIO device {} added to VM {}", config.id, req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to add VFIO device {} to VM {}: {}",
                    config.id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn remove_vfio_device(
        &self,
        request: Request<RemoveDeviceRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Removing VFIO device {} from VM: {}",
            req.device_id, req.vm_id
        );

        match self.manager.remove_device(&req.vm_id, &req.device_id).await {
            Ok(()) => {
                info!(
                    "VFIO device {} removed from VM {}",
                    req.device_id, req.vm_id
                );
                Ok(Response::new(()))
            }
            Err(e) => {
                error!(
                    "Failed to remove VFIO device {} from VM {}: {}",
                    req.device_id, req.vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn resize_vm(&self, request: Request<ResizeVmRequest>) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        info!(
            "Resizing VM {}: vcpus={:?} ram={:?}",
            req.vm_id, req.desired_vcpus, req.desired_ram
        );

        match self
            .manager
            .resize_vm(&req.vm_id, req.desired_vcpus, req.desired_ram)
            .await
        {
            Ok(()) => {
                info!("VM {} resized successfully", req.vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to resize VM {}: {}", req.vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn pull_image(
        &self,
        request: Request<OciImageRequest>,
    ) -> Result<Response<OciImageResponse>, Status> {
        let image_ref = request.into_inner().image_ref;
        info!("Pulling OCI image: {}", image_ref);

        let store = self
            .manager
            .image_store_manager()
            .ok_or_else(|| Status::unimplemented("virtiofsd not configured on this node"))?;

        match store.pull_and_unpack(&image_ref).await {
            Ok(info) => Ok(Response::new(OciImageResponse {
                image_ref: info.image_ref,
                digest: info.digest,
                bootstrap_path: info.rootfs_path.to_string_lossy().to_string(),
                socket_path: String::new(),
                available: true,
            })),
            Err(e) => {
                error!("Failed to pull image {}: {}", image_ref, e);
                Err(Status::internal(format!("Pull failed: {}", e)))
            }
        }
    }

    async fn get_image_status(
        &self,
        request: Request<OciImageRequest>,
    ) -> Result<Response<OciImageResponse>, Status> {
        let image_ref = request.into_inner().image_ref;
        info!("Getting image status: {}", image_ref);

        let store = self
            .manager
            .image_store_manager()
            .ok_or_else(|| Status::unimplemented("virtiofsd not configured on this node"))?;

        match store.get_image_status(&image_ref).await {
            Some(info) => Ok(Response::new(OciImageResponse {
                image_ref: info.image_ref,
                digest: info.digest,
                bootstrap_path: info.rootfs_path.to_string_lossy().to_string(),
                socket_path: String::new(),
                available: true,
            })),
            None => Ok(Response::new(OciImageResponse {
                image_ref,
                digest: String::new(),
                bootstrap_path: String::new(),
                socket_path: String::new(),
                available: false,
            })),
        }
    }

    async fn import_overlay_bd_image(
        &self,
        request: Request<ImportOverlayBdRequest>,
    ) -> Result<Response<ImportOverlayBdResponse>, Status> {
        let req = request.into_inner();
        info!(
            "Importing OverlayBD image: {} to registry {}",
            req.image_ref, req.registry_url
        );

        let obd_manager = self
            .manager
            .overlaybd_manager()
            .ok_or_else(|| Status::unimplemented("OverlayBD not configured on this node"))?;

        match obd_manager
            .import_image(&req.image_ref, &req.registry_url)
            .await
        {
            Ok(target_ref) => {
                info!("OverlayBD image imported: {}", target_ref);
                Ok(Response::new(ImportOverlayBdResponse {
                    image_ref: target_ref,
                    digest: String::new(), // digest resolved by node at mount time
                    available: true,
                }))
            }
            Err(e) => {
                error!("Failed to import OverlayBD image {}: {}", req.image_ref, e);
                Err(Status::internal(format!("OverlayBD import failed: {}", e)))
            }
        }
    }

    async fn read_console_log(
        &self,
        request: Request<VmId>,
    ) -> Result<Response<ConsoleLogResponse>, Status> {
        let vm_id = request.into_inner().id;
        info!("Reading console log for VM: {}", vm_id);

        let log_path = self
            .manager
            .runtime_dir()
            .join(format!("{}.console.log", vm_id));

        if !log_path.exists() {
            return Ok(Response::new(ConsoleLogResponse {
                content: String::new(),
                available: false,
            }));
        }

        match tokio::fs::read_to_string(&log_path).await {
            Ok(content) => Ok(Response::new(ConsoleLogResponse {
                content,
                available: true,
            })),
            Err(e) => {
                error!("Failed to read console log for VM {}: {}", vm_id, e);
                Err(Status::internal(format!(
                    "Failed to read console log: {}",
                    e
                )))
            }
        }
    }

    #[instrument(skip(self, _request))]
    async fn get_node_info(&self, _request: Request<()>) -> Result<Response<NodeInfo>, Status> {
        let hostname = gethostname::gethostname().to_string_lossy().into_owned();

        // Get Cloud Hypervisor version
        let ch_version = match tokio::process::Command::new(self.manager.ch_binary())
            .arg("--version")
            .output()
            .await
        {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
            Err(e) => {
                error!("Failed to get Cloud Hypervisor version: {}", e);
                "unknown".to_string()
            }
        };

        // Get kernel version
        let kernel_version = tokio::fs::read_to_string("/proc/version")
            .await
            .unwrap_or_else(|_| "unknown".to_string())
            .split_whitespace()
            .nth(2)
            .unwrap_or("unknown")
            .to_string();

        // Resource info
        let total_cpus = num_cpus::get() as i32;

        let (total_memory_bytes, available_memory_bytes) = parse_meminfo().await;

        let load_average_1m = parse_loadavg().await;

        let (disk_total_bytes, disk_available_bytes) = disk_usage(self.manager.runtime_dir());

        let gpus = discover_gpus().await;

        Ok(Response::new(NodeInfo {
            hostname,
            cloud_hypervisor_version: ch_version,
            kernel_version,
            total_cpus,
            total_memory_bytes,
            available_memory_bytes,
            load_average_1m,
            disk_total_bytes,
            disk_available_bytes,
            gpus,
            // Used in e2e tests to simulate deploying a different version.
            node_version: std::env::var("QARAX_NODE_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
        }))
    }

    async fn attach_console(
        &self,
        request: Request<tonic::Streaming<ConsoleInput>>,
    ) -> Result<Response<Self::AttachConsoleStream>, Status> {
        let mut input_stream = request.into_inner();

        // Read first message to get VM ID
        let first_msg = input_stream
            .message()
            .await
            .map_err(|e| Status::invalid_argument(format!("Failed to read first message: {}", e)))?
            .ok_or_else(|| Status::invalid_argument("Empty input stream"))?;

        let vm_id = first_msg.vm_id.clone();
        info!("Attaching to console for VM: {}", vm_id);

        // Get PTY path from VM manager
        let pty_path = self
            .manager
            .get_serial_pty_path(&vm_id)
            .await
            .map_err(|e| {
                error!("Failed to get PTY path for VM {}: {}", vm_id, e);
                map_manager_error(e)
            })?
            .ok_or_else(|| {
                Status::failed_precondition("VM console is not configured for PTY mode")
            })?;

        info!("Opening PTY for VM {}: {}", vm_id, pty_path);

        // Verify the PTY device still exists before trying to open it.
        // When the Cloud Hypervisor process exits, the kernel removes the PTY
        // slave device, but we may still have the path cached.
        if !std::path::Path::new(&pty_path).exists() {
            let is_running = self.manager.is_vm_process_alive(&vm_id).await;
            error!(
                "PTY {} for VM {} does not exist (process alive: {})",
                pty_path, vm_id, is_running
            );
            return Err(Status::failed_precondition(format!(
                "VM console PTY {} no longer exists — the VM process may have crashed",
                pty_path
            )));
        }

        // Open two independent file descriptors for the PTY slave — one for
        // reading (VM output) and one for writing (user input).
        //
        // Using a single tokio::fs::File with tokio::io::split does NOT work
        // here: tokio::fs::File serialises all I/O through one file handle via
        // spawn_blocking.  When the read task is blocked in the thread pool
        // waiting for VM output, the write task cannot acquire the handle and
        // deadlocks — the user's keystrokes never reach the VM.  Two separate
        // fds solve this: reads and writes run independently.
        let pty_path_read = pty_path.clone();
        let pty_read_std = tokio::task::spawn_blocking(move || {
            std::fs::OpenOptions::new().read(true).open(&pty_path_read)
        })
        .await
        .map_err(|e| {
            error!("spawn_blocking join error: {}", e);
            Status::internal(format!("Failed to open PTY: {}", e))
        })?
        .map_err(|e| {
            error!("Failed to open PTY {} for reading: {}", pty_path, e);
            Status::internal(format!("Failed to open PTY {}: {}", pty_path, e))
        })?;

        // Put the PTY slave into raw mode (termios is device-wide, so one
        // tcsetattr call covers both fds).
        {
            use nix::sys::termios::{self, SetArg};
            use std::os::unix::io::AsFd;
            match termios::tcgetattr(pty_read_std.as_fd()) {
                Ok(mut t) => {
                    termios::cfmakeraw(&mut t);
                    if let Err(e) = termios::tcsetattr(pty_read_std.as_fd(), SetArg::TCSANOW, &t) {
                        warn!("Failed to set PTY to raw mode: {}", e);
                    }
                }
                Err(e) => warn!("Failed to get PTY termios: {}", e),
            }
        }

        let pty_path_write = pty_path.clone();
        let pty_write_std = tokio::task::spawn_blocking(move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&pty_path_write)
        })
        .await
        .map_err(|e| {
            error!("spawn_blocking join error: {}", e);
            Status::internal(format!("Failed to open PTY: {}", e))
        })?
        .map_err(|e| {
            error!("Failed to open PTY {} for writing: {}", pty_path, e);
            Status::internal(format!("Failed to open PTY: {}", e))
        })?;

        let pty_reader = tokio::fs::File::from(pty_read_std);
        let mut pty_writer = tokio::fs::File::from(pty_write_std);

        // Channel for sending output to client
        let (tx, rx) = tokio::sync::mpsc::channel(128);

        // Spawn task to read from PTY and send to client
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(pty_reader);
            let mut buffer = vec![0u8; 4096];

            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => {
                        debug!("PTY read EOF");
                        break;
                    }
                    Ok(n) => {
                        let output = ConsoleOutput {
                            data: buffer[..n].to_vec(),
                            error: false,
                            error_message: String::new(),
                        };
                        if tx_clone.send(Ok(output)).await.is_err() {
                            debug!("Client disconnected");
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("PTY read error: {}", e);
                        let error_output = ConsoleOutput {
                            data: vec![],
                            error: true,
                            error_message: format!("PTY read error: {}", e),
                        };
                        let _ = tx_clone.send(Ok(error_output)).await;
                        break;
                    }
                }
            }
        });

        // Spawn task to read from client and write to PTY
        tokio::spawn(async move {
            // Process first message if it has data
            if !first_msg.data.is_empty()
                && let Err(e) = pty_writer.write_all(&first_msg.data).await
            {
                warn!("Failed to write to PTY: {}", e);
                return;
            }

            // Process remaining messages
            while let Ok(Some(msg)) = input_stream.message().await {
                if msg.resize {
                    // Handle terminal resize (would need ioctl TIOCSWINSZ)
                    debug!("Terminal resize requested: {}x{}", msg.cols, msg.rows);
                    // TODO: Implement terminal resize using nix crate
                } else if !msg.data.is_empty() {
                    if let Err(e) = pty_writer.write_all(&msg.data).await {
                        warn!("Failed to write to PTY: {}", e);
                        break;
                    }
                    if let Err(e) = pty_writer.flush().await {
                        warn!("Failed to flush PTY: {}", e);
                        break;
                    }
                }
            }
            debug!("Console input stream ended");
        });

        let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(
            Box::pin(output_stream) as Self::AttachConsoleStream
        ))
    }

    async fn get_console_pty_path(
        &self,
        request: Request<VmId>,
    ) -> Result<Response<ConsolePtyPathResponse>, Status> {
        let vm_id = request.into_inner().id;
        info!("Getting console PTY path for VM: {}", vm_id);

        match self.manager.get_serial_pty_path(&vm_id).await {
            Ok(Some(pty_path)) => Ok(Response::new(ConsolePtyPathResponse {
                pty_path,
                available: true,
            })),
            Ok(None) => Ok(Response::new(ConsolePtyPathResponse {
                pty_path: String::new(),
                available: false,
            })),
            Err(e) => {
                error!("Failed to get PTY path for VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    async fn attach_storage_pool(
        &self,
        request: Request<AttachStoragePoolRequest>,
    ) -> Result<Response<AttachStoragePoolResponse>, Status> {
        let req = request.into_inner();
        let pool_id = &req.pool_id;
        info!(
            "Attaching storage pool {} (kind={:?})",
            pool_id, req.pool_kind
        );

        let kind = StoragePoolKind::try_from(req.pool_kind).unwrap_or(StoragePoolKind::Local);

        let result = match kind {
            StoragePoolKind::Local => attach_local_pool(pool_id, &req.config_json).await,
            StoragePoolKind::Nfs => attach_nfs_pool(pool_id, &req.config_json).await,
            StoragePoolKind::Overlaybd => check_overlaybd_registry(&req.config_json).await,
        };

        match result {
            Ok(msg) => {
                info!("Storage pool {} attached: {}", pool_id, msg);
                Ok(Response::new(AttachStoragePoolResponse {
                    success: true,
                    message: msg,
                }))
            }
            Err(e) => {
                error!("Failed to attach storage pool {}: {}", pool_id, e);
                Ok(Response::new(AttachStoragePoolResponse {
                    success: false,
                    message: e.to_string(),
                }))
            }
        }
    }

    async fn attach_network(
        &self,
        request: Request<AttachNetworkRequest>,
    ) -> Result<Response<AttachNetworkResponse>, Status> {
        let req = request.into_inner();
        info!("Attaching network bridge: {}", req.bridge_name);

        let bridged = !req.parent_interface.is_empty();

        if bridged {
            // Bridged mode: bridge an existing NIC (its IP moves to the bridge)
            crate::networking::bridge::bridge_interface(&req.bridge_name, &req.parent_interface)
                .await
                .map_err(|e| Status::internal(format!("Failed to bridge interface: {}", e)))?;
        } else {
            // Isolated mode: create a new bridge with its own gateway IP
            let prefix = req.subnet.split_once('/').map(|(_, p)| p).unwrap_or("24");
            let gateway_cidr = format!("{}/{}", req.gateway, prefix);

            crate::networking::bridge::create_bridge(&req.bridge_name)
                .await
                .map_err(|e| Status::internal(format!("Failed to create bridge: {}", e)))?;

            crate::networking::bridge::set_bridge_ip(&req.bridge_name, &gateway_cidr)
                .await
                .map_err(|e| Status::internal(format!("Failed to set bridge IP: {}", e)))?;
        }

        // Start DHCP server (both modes need DHCP for guest VMs)
        let dns = if req.dns.is_empty() {
            &req.gateway
        } else {
            &req.dns
        };
        crate::networking::dhcp::start_dhcp_server(
            &req.bridge_name,
            &req.dhcp_range_start,
            &req.dhcp_range_end,
            &req.gateway,
            dns,
        )
        .await
        .map_err(|e| Status::internal(format!("Failed to start DHCP server: {}", e)))?;

        // NAT is only needed in isolated mode — bridged mode shares the upstream network
        if !bridged {
            crate::networking::nftables::setup_nat(&req.bridge_name, &req.subnet)
                .await
                .map_err(|e| Status::internal(format!("Failed to setup NAT: {}", e)))?;
        }

        info!(
            "Network bridge {} attached successfully (bridged={})",
            req.bridge_name, bridged
        );
        Ok(Response::new(AttachNetworkResponse {}))
    }

    async fn detach_network(
        &self,
        request: Request<DetachNetworkRequest>,
    ) -> Result<Response<DetachNetworkResponse>, Status> {
        let req = request.into_inner();
        info!("Detaching network bridge: {}", req.bridge_name);

        // Stop DHCP server (both modes run it for DHCP)
        if let Err(e) = crate::networking::dhcp::stop_dhcp_server(&req.bridge_name).await {
            warn!("Failed to stop DHCP server for {}: {}", req.bridge_name, e);
        }

        if crate::networking::bridge::is_bridged_interface(&req.bridge_name).await {
            // Bridged mode: move IP back to parent NIC and delete bridge
            if let Err(e) = crate::networking::bridge::unbridge_interface(&req.bridge_name).await {
                warn!("Failed to unbridge {}: {}", req.bridge_name, e);
            }
        } else {
            // Isolated mode: delete bridge
            if let Err(e) = crate::networking::bridge::delete_bridge(&req.bridge_name).await {
                warn!("Failed to delete bridge {}: {}", req.bridge_name, e);
            }
        }

        info!("Network bridge {} detached", req.bridge_name);
        Ok(Response::new(DetachNetworkResponse {}))
    }

    async fn detach_storage_pool(
        &self,
        request: Request<DetachStoragePoolRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        let pool_id = &req.pool_id;
        info!(
            "Detaching storage pool {} (kind={:?})",
            pool_id, req.pool_kind
        );

        let kind = StoragePoolKind::try_from(req.pool_kind).unwrap_or(StoragePoolKind::Local);

        if kind == StoragePoolKind::Nfs {
            // Local and OverlayBD: nothing to undo on detach
            if let Err(e) = detach_nfs_pool(pool_id).await {
                error!("Failed to unmount NFS pool {}: {}", pool_id, e);
                return Err(Status::internal(format!("NFS umount failed: {}", e)));
            }
        }

        Ok(Response::new(()))
    }

    async fn receive_migration(
        &self,
        request: Request<ReceiveMigrationRequest>,
    ) -> Result<Response<ReceiveMigrationResponse>, Status> {
        let req = request.into_inner();
        let vm_id = req.vm_id.clone();
        info!("Receiving migration for VM: {}", vm_id);

        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Missing VM config for receive_migration"))?;
        let port = req.migration_port as u16;

        match self.manager.receive_migration(&vm_id, config, port).await {
            Ok(receiver_url) => {
                info!(
                    "VM {} ready to receive migration at {}",
                    vm_id, receiver_url
                );
                Ok(Response::new(ReceiveMigrationResponse { receiver_url }))
            }
            Err(e) => {
                error!(
                    "Failed to prepare receive migration for VM {}: {}",
                    vm_id, e
                );
                Err(map_manager_error(e))
            }
        }
    }

    async fn send_migration(
        &self,
        request: Request<SendMigrationRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();
        let vm_id = req.vm_id.clone();
        info!(
            "Sending migration for VM {} to {}",
            vm_id, req.destination_url
        );

        match self
            .manager
            .send_migration(&vm_id, &req.destination_url)
            .await
        {
            Ok(()) => {
                info!("VM {} migrated out successfully", vm_id);
                Ok(Response::new(()))
            }
            Err(e) => {
                error!("Failed to send migration for VM {}: {}", vm_id, e);
                Err(map_manager_error(e))
            }
        }
    }

    type AttachConsoleStream =
        Pin<Box<dyn Stream<Item = Result<ConsoleOutput, Status>> + Send + 'static>>;
}
// ─── Storage pool attachment helpers ─────────────────────────────────────────

/// Ensure the local directory for this pool exists.
async fn attach_local_pool(pool_id: &str, config_json: &str) -> AnyhowResult<String> {
    let cfg: serde_json::Value =
        serde_json::from_str(config_json).unwrap_or_else(|_| serde_json::json!({}));

    // Fall back to a standard per-pool directory if config has no path.
    let path_str = cfg.get("path").and_then(|v| v.as_str()).unwrap_or_default();

    let dir = if path_str.is_empty() {
        std::path::PathBuf::from(format!("/var/lib/qarax/pools/{}", pool_id))
    } else {
        std::path::PathBuf::from(path_str)
    };

    tokio::fs::create_dir_all(&dir).await?;
    Ok(format!("local dir {} ready", dir.display()))
}

/// Mount an NFS export for this pool.
/// Validate an NFS URL of the form `host:/path`.
///
/// Rejects blank values, missing colon-slash separator, and shell-injectable
/// characters that have no place in a hostname or export path.
fn validate_nfs_url(url: &str) -> AnyhowResult<()> {
    let (host, path) = url
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid NFS URL {url:?}: expected 'host:/path'"))?;

    if host.is_empty() {
        anyhow::bail!("Invalid NFS URL {url:?}: host is empty");
    }
    if !path.starts_with('/') {
        anyhow::bail!("Invalid NFS URL {url:?}: path must start with '/'");
    }

    let bad = |c: char| matches!(c, '\0' | '\n' | '\r' | ';' | '&' | '|' | '`' | '$' | '\\');
    if host.chars().any(bad) || path.chars().any(bad) {
        anyhow::bail!("Invalid NFS URL {url:?}: contains illegal characters");
    }

    Ok(())
}

async fn attach_nfs_pool(pool_id: &str, config_json: &str) -> AnyhowResult<String> {
    let cfg: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|e| anyhow::anyhow!("Invalid NFS pool config JSON: {}", e))?;

    let url = cfg
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("NFS pool config missing 'url' field"))?;

    validate_nfs_url(url)?;

    let mount_point = format!("/var/lib/qarax/pools/{}", pool_id);
    tokio::fs::create_dir_all(&mount_point).await?;

    let output = tokio::process::Command::new("mount")
        .args(["-t", "nfs", "-o", "nolock", url, &mount_point])
        .output()
        .await?;

    if output.status.success() {
        Ok(format!("NFS {} mounted at {}", url, mount_point))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("mount failed: {}", stderr.trim()))
    }
}

/// Unmount the NFS export for this pool.
async fn detach_nfs_pool(pool_id: &str) -> AnyhowResult<()> {
    let mount_point = format!("/var/lib/qarax/pools/{}", pool_id);

    let output = tokio::process::Command::new("umount")
        .arg(&mount_point)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("umount failed: {}", stderr.trim()))
    }
}

/// Verify that an OverlayBD registry is reachable at the configured URL.
async fn check_overlaybd_registry(config_json: &str) -> AnyhowResult<String> {
    let cfg: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|e| anyhow::anyhow!("Invalid OverlayBD pool config JSON: {}", e))?;

    let url = cfg
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("OverlayBD pool config missing 'url' field"))?;

    // Probe the OCI registry v2 endpoint.
    let probe = format!("{}/v2/", url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;
    let response = client
        .get(&probe)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Cannot reach registry at {}: {}", probe, e))?;

    // A v2 registry returns 200 or 401 (auth required); both mean it is alive.
    let status = response.status();
    if status.is_success() || status.as_u16() == 401 {
        Ok(format!("OverlayBD registry {} reachable ({})", url, status))
    } else {
        Err(anyhow::anyhow!(
            "OverlayBD registry {} returned unexpected status {}",
            url,
            status
        ))
    }
}

/// Discover GPUs bound to vfio-pci by scanning /sys/bus/pci/devices.
// PCI class codes for display controllers
const PCI_CLASS_VGA: &str = "0x030000"; // VGA compatible controller
const PCI_CLASS_3D: &str = "0x030200"; // 3D controller

// PCI vendor IDs
const PCI_VENDOR_NVIDIA: &str = "0x10de";
const PCI_VENDOR_AMD: &str = "0x1002";
const PCI_VENDOR_INTEL: &str = "0x8086";

async fn discover_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();

    let devices_dir = std::path::Path::new("/sys/bus/pci/devices");
    let entries = match tokio::fs::read_dir(devices_dir).await {
        Ok(e) => e,
        Err(_) => return gpus,
    };

    let mut entries = entries;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dev_path = entry.path();

        // Check PCI class — VGA (0x030000) or 3D controller (0x030200)
        let class_path = dev_path.join("class");
        let class_str = match tokio::fs::read_to_string(&class_path).await {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue,
        };
        if class_str != PCI_CLASS_VGA && class_str != PCI_CLASS_3D {
            continue;
        }

        // Check device is bound to vfio-pci
        let driver_link = dev_path.join("driver");
        match tokio::fs::read_link(&driver_link).await {
            Ok(target) => {
                let driver_name = target.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if driver_name != "vfio-pci" {
                    continue;
                }
            }
            Err(_) => continue,
        }

        let pci_address = entry.file_name().to_string_lossy().to_string();

        // Read vendor ID
        let vendor_id = tokio::fs::read_to_string(dev_path.join("vendor"))
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let vendor = match vendor_id.as_str() {
            PCI_VENDOR_NVIDIA => "nvidia".to_string(),
            PCI_VENDOR_AMD => "amd".to_string(),
            PCI_VENDOR_INTEL => "intel".to_string(),
            _ => vendor_id.clone(),
        };

        // Read device ID for model name
        let device_id = tokio::fs::read_to_string(dev_path.join("device"))
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let model = format!("{}:{}", vendor_id, device_id);

        // Read IOMMU group
        let iommu_group = match tokio::fs::read_link(dev_path.join("iommu_group")).await {
            Ok(target) => target
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(-1),
            Err(_) => -1,
        };

        // Estimate VRAM from BAR sizes in the resource file
        let vram_bytes = estimate_vram(&dev_path).await;

        gpus.push(GpuInfo {
            pci_address,
            model,
            vendor,
            vram_bytes,
            iommu_group,
        });
    }

    gpus
}

/// Estimate GPU VRAM by summing large BARs from the PCI resource file.
async fn estimate_vram(dev_path: &std::path::Path) -> i64 {
    let resource_content = match tokio::fs::read_to_string(dev_path.join("resource")).await {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mut max_bar: i64 = 0;
    for line in resource_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3
            && let (Ok(start), Ok(end)) = (
                i64::from_str_radix(parts[0].trim_start_matches("0x"), 16),
                i64::from_str_radix(parts[1].trim_start_matches("0x"), 16),
            )
        {
            let size = end.saturating_sub(start) + 1;
            if size > max_bar {
                max_bar = size;
            }
        }
    }
    max_bar
}

/// Parse /proc/meminfo to get total and available memory in bytes.
async fn parse_meminfo() -> (i64, i64) {
    let content = match tokio::fs::read_to_string("/proc/meminfo").await {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };
    let mut total: i64 = 0;
    let mut available: i64 = 0;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_meminfo_kb(rest) * 1024;
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = parse_meminfo_kb(rest) * 1024;
        }
    }
    (total, available)
}

fn parse_meminfo_kb(s: &str) -> i64 {
    s.trim()
        .strip_suffix("kB")
        .unwrap_or(s)
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
}

/// Parse 1-minute load average from /proc/loadavg.
async fn parse_loadavg() -> f64 {
    tokio::fs::read_to_string("/proc/loadavg")
        .await
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}

/// Get total and available disk bytes for a path via statvfs.
fn disk_usage(path: &std::path::Path) -> (i64, i64) {
    match nix::sys::statvfs::statvfs(path) {
        Ok(stat) => {
            let total = stat.blocks() as i64 * stat.fragment_size() as i64;
            let available = stat.blocks_available() as i64 * stat.fragment_size() as i64;
            (total, available)
        }
        Err(_) => (0, 0),
    }
}

fn map_manager_error(e: crate::cloud_hypervisor::VmManagerError) -> Status {
    use crate::cloud_hypervisor::VmManagerError;

    match e {
        VmManagerError::VmNotFound(id) => Status::not_found(format!("VM {} not found", id)),
        VmManagerError::VmAlreadyExists(id) => {
            Status::already_exists(format!("VM {} already exists", id))
        }
        VmManagerError::InvalidConfig(msg) => Status::invalid_argument(msg),
        VmManagerError::SpawnError(e) => Status::internal(format!("Failed to spawn CH: {}", e)),
        VmManagerError::SdkError(e) => {
            Status::internal(format!("Cloud Hypervisor SDK error: {}", e))
        }
        VmManagerError::ProcessError(msg) => Status::internal(msg),
        VmManagerError::TapError(msg) => Status::internal(format!("TAP device error: {}", msg)),
        VmManagerError::OverlayBdError(e) => Status::internal(format!("OverlayBD error: {}", e)),
        VmManagerError::MigrationError(msg) => {
            Status::internal(format!("Migration error: {}", msg))
        }
    }
}
