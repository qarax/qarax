use super::*;
use crate::vmm::testing::{PAST_VMM_ANSWER_TIMEOUT, serve_vmm};
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
        .insert(vm_id.to_string(), ManagedVm::new(instance));
}

/// A Firecracker that stops answering holds up the operations on its own VM
/// only: those on the node's other VMs go on, and so does listing the node's
/// VMs, which its firewall, DHCP and probes read.
#[tokio::test]
async fn a_vmm_that_does_not_answer_holds_up_only_its_own_vm() {
    let directory = TempDir::new().unwrap();
    let manager = Arc::new(test_manager(&directory));
    let hung = uuid::Uuid::new_v4().to_string();
    let answering = uuid::Uuid::new_v4().to_string();

    let asked = serve_vmm(
        UnixListener::bind(manager.socket_path(&hung)).unwrap(),
        watch::channel(None).1,
    );
    serve_fake_firecracker(
        UnixListener::bind(manager.socket_path(&answering)).unwrap(),
        instance_info(FcInstanceState::Paused),
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

fn instance_info(state: FcInstanceState) -> String {
    serde_json::to_string(&InstanceInfo {
        state,
        ..Default::default()
    })
    .unwrap()
}

fn test_manager(directory: &TempDir) -> FirecrackerManager {
    FirecrackerManager::new(directory.path(), "/bin/true", Duration::from_secs(1), None)
}

/// A status query to a Firecracker that doesn't answer is given up, and the
/// VM is reported as it last was. That leaves nothing broken for the next
/// query, which sees Firecracker's state once it answers again.
#[tokio::test]
async fn status_of_a_vm_whose_vmm_does_not_answer_is_its_last_known_one() {
    let directory = TempDir::new().unwrap();
    let manager = test_manager(&directory);
    let vm_id = uuid::Uuid::new_v4().to_string();
    let (answer, answers) = watch::channel(None);
    serve_vmm(
        UnixListener::bind(manager.socket_path(&vm_id)).unwrap(),
        answers,
    );
    add_running_vm(&manager, &vm_id).await;

    let info = tokio::time::timeout(PAST_VMM_ANSWER_TIMEOUT, manager.get_vm_info(&vm_id))
        .await
        .expect("asking for the VM's status waited for a Firecracker that does not answer")
        .unwrap();
    assert_eq!(info.status(), VmStatus::Running);

    answer.send_replace(Some(instance_info(FcInstanceState::Paused)));
    let info = tokio::time::timeout(PAST_VMM_ANSWER_TIMEOUT, manager.get_vm_info(&vm_id))
        .await
        .expect("asking for the VM's status again waited")
        .unwrap();
    assert_eq!(info.status(), VmStatus::Paused);
}

/// Make `vm_id` one of `manager`'s VMs, running, on a Firecracker that has
/// stopped answering, and have the VM monitor's status query of it in flight.
async fn add_vm_whose_vmm_does_not_answer(
    manager: &Arc<FirecrackerManager>,
    vm_id: &str,
) -> tokio::task::JoinHandle<Result<VmState, VmmError>> {
    let asked = serve_vmm(
        UnixListener::bind(manager.socket_path(vm_id)).unwrap(),
        watch::channel(None).1,
    );
    add_running_vm(manager, vm_id).await;
    let monitor = tokio::spawn({
        let (manager, vm_id) = (manager.clone(), vm_id.to_string());
        async move { manager.get_vm_info(&vm_id).await }
    });
    asked.notified().await;
    monitor
}

/// Force-stopping a VM whose Firecracker doesn't answer asks it nothing: it
/// waits at most for a status query in flight to be given up.
#[tokio::test]
async fn a_vm_whose_vmm_does_not_answer_can_be_force_stopped() {
    let directory = TempDir::new().unwrap();
    let manager = Arc::new(test_manager(&directory));
    let vm_id = uuid::Uuid::new_v4().to_string();
    let monitor = add_vm_whose_vmm_does_not_answer(&manager, &vm_id).await;

    tokio::time::timeout(PAST_VMM_ANSWER_TIMEOUT, manager.force_stop_vm(&vm_id))
        .await
        .expect("force-stopping the VM waited for its Firecracker")
        .unwrap();
    assert_eq!(
        manager.lock_vm(&vm_id).await.unwrap().status(),
        VmStatus::Shutdown
    );
    monitor.abort();
}

/// Deleting a VM whose Firecracker doesn't answer asks it nothing either: it
/// waits at most for a status query in flight to be given up, then cleans up
/// after the VM.
#[tokio::test]
async fn a_vm_whose_vmm_does_not_answer_can_be_deleted() {
    let directory = TempDir::new().unwrap();
    let manager = Arc::new(test_manager(&directory));
    let vm_id = uuid::Uuid::new_v4().to_string();
    let monitor = add_vm_whose_vmm_does_not_answer(&manager, &vm_id).await;

    tokio::time::timeout(PAST_VMM_ANSWER_TIMEOUT, manager.delete_vm(&vm_id))
        .await
        .expect("deleting the VM waited for its Firecracker")
        .unwrap();
    assert!(manager.list_vms().await.is_empty());
    assert!(!manager.socket_path(&vm_id).exists());
    monitor.abort();
}

/// A Firecracker that doesn't answer doesn't hold up the node's startup: its
/// VM is recovered with an unknown status.
#[tokio::test]
async fn a_vm_whose_vmm_does_not_answer_is_recovered_with_an_unknown_status() {
    let directory = TempDir::new().unwrap();
    let manager = test_manager(&directory);
    let vm_id = uuid::Uuid::new_v4().to_string();
    let config = ProtoVmConfig {
        vm_id: vm_id.clone(),
        ..Default::default()
    };
    tokio::fs::write(manager.config_path(&vm_id), config.encode_to_vec())
        .await
        .unwrap();
    serve_vmm(
        UnixListener::bind(manager.socket_path(&vm_id)).unwrap(),
        watch::channel(None).1,
    );

    tokio::time::timeout(PAST_VMM_ANSWER_TIMEOUT, manager.recover_vms())
        .await
        .expect("recovering the node's VMs waited for a Firecracker that does not answer");
    let recovered = manager.list_vms().await;
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].status(), VmStatus::Unknown);
}

/// Of concurrent adds of one VM id (creates, a restore), one goes ahead and
/// the others are refused before they make anything. The id is free again
/// once that add fails, or is cancelled as when its request is dropped, and a
/// create of it then adds the VM.
#[tokio::test]
async fn a_vm_id_is_added_once_at_a_time() {
    let directory = TempDir::new().unwrap();
    // Its VMM, /bin/true, exits without serving the API socket: an add waits
    // for the socket, then fails.
    let manager = test_manager(&directory);
    let vm_id = uuid::Uuid::new_v4().to_string();
    let config = ProtoVmConfig {
        vm_id: vm_id.clone(),
        ..Default::default()
    };

    let (created, created_again, restored) = tokio::join!(
        manager.create_vm(config.clone()),
        manager.create_vm(config.clone()),
        manager.restore_vm(&vm_id, "file:///no-snapshot"),
    );
    let results = [created.map(drop), created_again.map(drop), restored];
    let refused = results
        .iter()
        .filter(|result| matches!(result, Err(VmmError::VmAlreadyExists(_))))
        .count();
    assert_eq!(refused, results.len() - 1, "{results:?}");

    // Not refused, so still waiting for its VMM when it is cancelled.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            manager.create_vm(config.clone())
        )
        .await
        .is_err(),
        "a create was refused after the add of its id failed"
    );

    let log = manager.log_path(&vm_id);
    std::fs::remove_file(&log).unwrap();
    let socket = manager.socket_path(&vm_id);
    let (created, ()) = tokio::join!(manager.create_vm(config), async {
        // The create opens its VMM's log before spawning it.
        while !log.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        serve_fake_firecracker(
            UnixListener::bind(&socket).unwrap(),
            instance_info(FcInstanceState::NotStarted),
        );
    });
    created.expect("a create of a VM whose VMM serves its API failed");
    let listed: Vec<String> = manager
        .list_vms()
        .await
        .into_iter()
        .map(|vm| vm.config.unwrap().vm_id)
        .collect();
    assert_eq!(listed, [vm_id]);
}
