use super::*;
use firecracker_rust_sdk::models::InstanceInfo;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

/// Answers every Firecracker API request on `listener` with `body`.
fn serve_fake_firecracker(listener: UnixListener, body: String) {
    let body = Arc::new(body);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body = body.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                // One connection carries all of an SDK handle's requests.
                while let Ok(1..) = stream.read(&mut request).await {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            });
        }
    });
}

/// Make `vm_id` one of `manager`'s VMs, running, on a Firecracker serving
/// its API on its socket.
async fn add_running_vm(manager: &FirecrackerManager, vm_id: &str) {
    let socket_path = manager.socket_path(vm_id);
    let micro_vm = Machine::attach(&socket_path)
        .await
        .unwrap()
        .assume_running();
    let instance = FcVmInstance::new(
        ProtoVmConfig {
            vm_id: vm_id.to_string(),
            ..Default::default()
        },
        None,
        socket_path,
        Vec::new(),
        FcPhase::Live {
            micro_vm,
            state: LiveState::Running,
            forking: ForkMemory::new(false),
        },
    );
    manager
        .vms
        .lock()
        .unwrap()
        .insert(vm_id.to_string(), ManagedVm::new(instance));
}

/// A Firecracker that stops answering holds up the operations on its own VM
/// only: those on the node's other VMs go on, and so does listing the node's
/// VMs, which its firewall, DHCP and probes read.
#[tokio::test]
async fn a_vmm_that_does_not_answer_holds_up_only_its_own_vm() {
    let directory = TempDir::new().unwrap();
    let manager = Arc::new(FirecrackerManager::new(
        directory.path(),
        "/bin/true",
        Duration::from_secs(1),
        None,
    ));
    let hung = uuid::Uuid::new_v4().to_string();
    let answering = uuid::Uuid::new_v4().to_string();

    // Takes requests and never answers them.
    let listener = UnixListener::bind(manager.socket_path(&hung)).unwrap();
    let asked = Arc::new(tokio::sync::Notify::new());
    let notify_asked = asked.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let asked = notify_asked.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                if stream.read(&mut request).await.unwrap() > 0 {
                    asked.notify_one();
                }
                std::future::pending::<()>().await;
            });
        }
    });
    serve_fake_firecracker(
        UnixListener::bind(manager.socket_path(&answering)).unwrap(),
        serde_json::to_string(&InstanceInfo {
            state: FcInstanceState::Paused,
            ..Default::default()
        })
        .unwrap(),
    );
    for vm_id in [&hung, &answering] {
        add_running_vm(&manager, vm_id).await;
    }

    let waiting = tokio::spawn({
        let (manager, hung) = (manager.clone(), hung.clone());
        async move { manager.get_vm_info(&hung).await }
    });
    asked.notified().await;

    // Far longer than either takes, even on a loaded machine.
    const PROMPTLY: Duration = Duration::from_secs(5);
    let info = tokio::time::timeout(PROMPTLY, manager.get_vm_info(&answering))
        .await
        .expect("asking for one VM's info waited for another VM's Firecracker")
        .unwrap();
    assert_eq!(info.status(), VmStatus::Paused);
    let listed = tokio::time::timeout(PROMPTLY, manager.list_vms())
        .await
        .expect("listing the node's VMs waited for a VM's Firecracker");
    let mut listed: Vec<(String, VmStatus)> = listed
        .into_iter()
        .map(|vm| {
            (
                vm.config.unwrap().vm_id,
                VmStatus::try_from(vm.status).unwrap(),
            )
        })
        .collect();
    listed.sort();
    // The VM whose Firecracker doesn't answer, as it last was.
    let mut expected = vec![(hung, VmStatus::Running), (answering, VmStatus::Paused)];
    expected.sort();
    assert_eq!(listed, expected);
    assert!(!waiting.is_finished());
    waiting.abort();
}
