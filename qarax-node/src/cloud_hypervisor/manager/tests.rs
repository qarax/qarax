use super::*;
use std::assert_matches;

use crate::rpc::node::{CpuPinning as ProtoCpuPinning, NumaPlacement as ProtoNumaPlacement};
use tempfile::TempDir;

fn test_manager(runtime_dir: &TempDir) -> VmManager {
    VmManager::with_storage(
        runtime_dir.path(),
        "/bin/true",
        StorageBackendRegistry::new(),
        None,
        None,
    )
}

fn base_vm_config(memory_size: i64, boot_vcpus: i32) -> VmConfig {
    VmConfig {
        cpus: Some(Box::new(CpusConfig {
            boot_vcpus,
            max_vcpus: boot_vcpus,
            ..Default::default()
        })),
        memory: Some(Box::new(MemoryConfig {
            size: memory_size,
            ..Default::default()
        })),
        ..Default::default()
    }
}

#[test]
fn apply_numa_placement_sets_cpu_affinity() {
    let mut config = base_vm_config(512 * 1024 * 1024, 2);
    let placement = ProtoNumaPlacement {
        host_numa_node_ids: vec![],
        cpu_pinning: vec![
            ProtoCpuPinning {
                vcpu: 0,
                host_cpus: vec![0, 1],
            },
            ProtoCpuPinning {
                vcpu: 1,
                host_cpus: vec![2, 3],
            },
        ],
        memory_zone_ids: vec![],
    };

    VmManager::apply_numa_placement(&mut config, &placement);

    let affinity = config.cpus.as_ref().unwrap().affinity.as_ref().unwrap();
    assert_eq!(affinity.len(), 2);
    assert_eq!(affinity[0].vcpu, 0);
    assert_eq!(affinity[0].host_cpus, vec![0, 1]);
    assert_eq!(affinity[1].vcpu, 1);
    assert_eq!(affinity[1].host_cpus, vec![2, 3]);
}

#[test]
fn apply_numa_placement_creates_memory_zones() {
    let mut config = base_vm_config(1024 * 1024 * 1024, 4);
    let placement = ProtoNumaPlacement {
        host_numa_node_ids: vec![0],
        cpu_pinning: vec![],
        memory_zone_ids: vec!["zone0".to_string()],
    };

    VmManager::apply_numa_placement(&mut config, &placement);

    let memory = config.memory.as_ref().unwrap();
    // When zones are set, size must be 0
    assert_eq!(memory.size, 0);
    let zones = memory.zones.as_ref().unwrap();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].id, "zone0");
    assert_eq!(zones[0].host_numa_node, Some(0));
}

#[test]
fn apply_numa_placement_preserves_memory_remainder() {
    let mut config = base_vm_config(1025, 4);
    let placement = ProtoNumaPlacement {
        host_numa_node_ids: vec![0, 1],
        cpu_pinning: vec![],
        memory_zone_ids: vec!["zone0".to_string(), "zone1".to_string()],
    };

    VmManager::apply_numa_placement(&mut config, &placement);

    let memory = config.memory.as_ref().unwrap();
    assert_eq!(memory.size, 0);
    let zones = memory.zones.as_ref().unwrap();
    assert_eq!(zones.len(), 2);
    assert_eq!(zones[0].size, 513);
    assert_eq!(zones[1].size, 512);
}

#[test]
fn apply_numa_placement_sets_guest_numa_topology() {
    let mut config = base_vm_config(512 * 1024 * 1024, 2);
    let placement = ProtoNumaPlacement {
        host_numa_node_ids: vec![0],
        cpu_pinning: vec![
            ProtoCpuPinning {
                vcpu: 0,
                host_cpus: vec![0],
            },
            ProtoCpuPinning {
                vcpu: 1,
                host_cpus: vec![1],
            },
        ],
        memory_zone_ids: vec!["zone0".to_string()],
    };

    VmManager::apply_numa_placement(&mut config, &placement);

    let numa = config.numa.as_ref().unwrap();
    assert_eq!(numa.len(), 1);
    assert_eq!(numa[0].guest_numa_id, 0);
    // All vCPUs (0..boot_vcpus) assigned to guest node 0
    assert_eq!(numa[0].cpus, Some(vec![0, 1]));
    assert_eq!(numa[0].memory_zones, Some(vec!["zone0".to_string()]));
}

#[test]
fn apply_numa_placement_empty_is_noop() {
    let mut config = base_vm_config(512 * 1024 * 1024, 1);
    let original_size = config.memory.as_ref().unwrap().size;
    let placement = ProtoNumaPlacement {
        host_numa_node_ids: vec![],
        cpu_pinning: vec![],
        memory_zone_ids: vec![],
    };

    VmManager::apply_numa_placement(&mut config, &placement);

    // Nothing should have changed
    assert!(config.cpus.as_ref().unwrap().affinity.is_none());
    assert!(config.memory.as_ref().unwrap().zones.is_none());
    assert_eq!(config.memory.as_ref().unwrap().size, original_size);
    assert!(config.numa.is_none());
}

#[test]
fn resolve_vsock_config_sets_defaults() {
    let runtime_dir = TempDir::new().unwrap();
    let manager = test_manager(&runtime_dir);
    let mut vsock = ProtoVsockConfig::default();

    manager.resolve_vsock_config("test-vm", &mut vsock);

    assert!(vsock.cid.is_some());
    assert_eq!(
        vsock.socket.as_deref(),
        Some(
            runtime_dir
                .path()
                .join("test-vm.vsock")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[tokio::test]
async fn exec_vm_rejects_empty_command() {
    let runtime_dir = TempDir::new().unwrap();
    let manager = test_manager(&runtime_dir);

    let err = manager
        .exec_vm("test-vm", Vec::new(), None)
        .await
        .unwrap_err();

    assert_matches!(err, VmManagerError::ExecInvalid(_));
}

#[test]
fn build_guest_exec_request_is_newline_framed_json() {
    let payload =
        VmManager::build_guest_exec_request(vec!["/bin/echo".into(), "hello".into()], Some(5))
            .unwrap();

    assert_eq!(payload.last(), Some(&b'\n'));

    let body = std::str::from_utf8(&payload[..payload.len() - 1]).unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["command"], serde_json::json!(["/bin/echo", "hello"]));
    assert_eq!(json["timeout_secs"], 5);
}

#[tokio::test]
async fn recovered_vm_deletion_cleans_overlaybd_state_and_preserves_external_upper() {
    use crate::storage::overlaybd::OverlayBdBackend;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    // Cover both historical device-path results and daemon status messages,
    // plus a plain disk VM that has no OverlayBD state.
    for result in [Some("/dev/sdz"), Some("success"), None] {
        let directory = TempDir::new().unwrap();
        let runtime = directory.path().join("vms");
        tokio::fs::create_dir(&runtime).await.unwrap();
        let cache = directory.path().join("overlaybd");
        let vm_id = Uuid::new_v4().to_string();
        let mapping_dir = cache.join(&vm_id);
        let upper = directory.path().join("persistent.upper.data");
        tokio::fs::write(&upper, b"guest data").await.unwrap();
        if let Some(result) = result {
            tokio::fs::create_dir_all(&mapping_dir).await.unwrap();
            tokio::fs::write(
                mapping_dir.join("config.json"),
                serde_json::json!({
                    "upper": {"data": upper}, "lowers": [],
                })
                .to_string(),
            )
            .await
            .unwrap();
            tokio::fs::write(mapping_dir.join("result"), result)
                .await
                .unwrap();
        }
        let overlaybd = Arc::new(OverlayBdManager::new("unused-convertor", &cache));
        overlaybd.recover().await;
        assert_eq!(overlaybd.has_mapping(&vm_id).await, result.is_some());
        let mut backends = StorageBackendRegistry::new();
        backends.register(
            StoragePoolKind::Overlaybd,
            Arc::new(OverlayBdBackend::new(overlaybd.clone(), None)),
        );
        let manager = VmManager::with_storage(
            &runtime,
            "unused-cloud-hypervisor",
            backends,
            Some(overlaybd.clone()),
            None,
        );
        let config = ProtoVmConfig {
            vm_id: vm_id.clone(),
            disks: vec![ProtoDiskConfig {
                id: "rootfs".into(),
                path: Some("/dev/sdz".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        tokio::fs::write(manager.config_path(&vm_id), config.encode_to_vec())
            .await
            .unwrap();
        let socket = manager.socket_path(&vm_id);
        let listener = UnixListener::bind(&socket).unwrap();
        let hypervisor = tokio::spawn(async move {
            // Recovery first checks that the socket answers, with a connection
            // it closes without a request; serve the one that sends requests.
            let (mut stream, mut pending) = loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let size = stream.read(&mut request).await.unwrap();
                if size > 0 {
                    request.truncate(size);
                    break (stream, Some(request));
                }
            };
            for expected in ["GET /api/v1/vm.info", "PUT /api/v1/vm.shutdown"] {
                let request = match pending.take() {
                    Some(request) => request,
                    None => {
                        let mut request = vec![0; 4096];
                        let size = stream.read(&mut request).await.unwrap();
                        request.truncate(size);
                        request
                    }
                };
                let request = String::from_utf8_lossy(&request);
                let (method, path) = expected.split_once(' ').unwrap();
                let mut parts = request.split_whitespace();
                assert_eq!(parts.next(), Some(method), "{request}");
                assert!(parts.next().unwrap().ends_with(path), "{request}");
                let response = if expected.starts_with("GET") {
                    let body = serde_json::to_string(&models::VmInfo::new(
                        VmConfig::default(),
                        models::VmState::Running,
                    ))
                    .unwrap();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".into()
                };
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        manager.recover_vms().await;
        {
            let vms = manager.vms.lock().await;
            let instance = vms.get(&vm_id).unwrap();
            assert_eq!(instance.status, VmStatus::Running);
            assert_eq!(
                instance.storage_backend_kind,
                result.map(|_| StoragePoolKind::Overlaybd)
            );
        }
        manager.delete_vm(&vm_id).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), hypervisor)
            .await
            .unwrap()
            .unwrap();
        assert!(!mapping_dir.exists());
        assert!(!overlaybd.has_mapping(&vm_id).await);
        assert!(!socket.exists());
        assert!(!manager.config_path(&vm_id).exists());
        assert_eq!(tokio::fs::read(&upper).await.unwrap(), b"guest data");
    }
}

#[test]
fn vfio_device_iommu_is_sent_as_iommu_type() {
    for (iommu, expected) in [
        (Some(true), Some("Virtio")),
        (Some(false), Some("Off")),
        (None, None),
    ] {
        let device = ProtoVfioDeviceConfig {
            id: "gpu0".into(),
            path: "/sys/bus/pci/devices/0000:01:00.0".into(),
            iommu,
            ..Default::default()
        };
        let json =
            serde_json::to_value(VmManager::proto_vfio_device_to_sdk(&device).unwrap()).unwrap();
        assert_eq!(
            json.get("iommu").and_then(|v| v.as_str()),
            expected,
            "{iommu:?}"
        );
        assert_eq!(json["path"], "/sys/bus/pci/devices/0000:01:00.0");
    }
}

#[test]
fn pci_segment_must_fit_in_i16() {
    assert_eq!(VmManager::pci_segment_to_sdk(None).unwrap(), None);
    assert_eq!(VmManager::pci_segment_to_sdk(Some(1)).unwrap(), Some(1));

    let disk = ProtoDiskConfig {
        id: "disk0".into(),
        pci_segment: Some(i32::from(i16::MAX) + 1),
        ..Default::default()
    };
    assert_matches!(
        VmManager::proto_disk_to_sdk(&disk),
        Err(VmManagerError::InvalidConfig(_))
    );
}

#[test]
fn serial_port_config_keeps_mode_and_target() {
    let serial = ProtoConsoleConfig {
        mode: ProtoConsoleMode::File as i32,
        file: Some("/var/log/vm-serial.log".into()),
        ..Default::default()
    };
    let json = serde_json::to_value(VmManager::proto_serial_to_sdk(&serial)).unwrap();
    assert_eq!(json["mode"], "File");
    assert_eq!(json["file"], "/var/log/vm-serial.log");
}
