use qarax::Backend;
use qarax::DriveService;
use qarax::HostService;
use qarax::KernelService;
use qarax::StorageService;
use qarax::VmService;

use std::sync::Arc;

fn main() {
    let host_service = Arc::new(HostService::new());
    let drive_service = Arc::new(DriveService::new());
    let kernel_service = Arc::new(KernelService::new());
    let storage_service = Arc::new(StorageService::new());

    let vm_service = Arc::new(VmService::new(
        Arc::clone(&host_service),
        Arc::clone(&drive_service),
        Arc::clone(&kernel_service),
    ));

    let backend: Backend = Backend {
        host_service: host_service,
        vm_service: vm_service,
        storage_service: storage_service,
        drive_service: drive_service,
        kernel_service: kernel_service,
    };

    qarax::rocket(backend).launch();
}
