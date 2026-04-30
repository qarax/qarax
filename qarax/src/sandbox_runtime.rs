use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    App,
    errors::Error,
    grpc_client::NodeClient,
    model::{
        hosts,
        sandboxes::NewSandbox,
        vm_templates,
        vms::{self, Hypervisor, NewVm, ResolvedNewVm},
    },
};

pub(crate) async fn resolve_sandbox_vm(
    env: &App,
    req: &NewSandbox,
) -> Result<ResolvedNewVm, Error> {
    let sandbox_hypervisor = vm_templates::get(env.pool(), req.vm_template_id)
        .await
        .map_err(Error::Sqlx)?
        .hypervisor
        .unwrap_or(Hypervisor::Firecracker);

    let new_vm = NewVm {
        name: req.name.clone(),
        tags: None,
        vm_template_id: Some(req.vm_template_id),
        instance_type_id: req.instance_type_id,
        hypervisor: Some(sandbox_hypervisor),
        architecture: None,
        boot_vcpus: None,
        max_vcpus: None,
        cpu_topology: None,
        kvm_hyperv: None,
        memory_size: None,
        memory_hotplug_size: None,
        memory_mergeable: None,
        memory_shared: None,
        memory_hugepages: None,
        memory_hugepage_size: None,
        memory_prefault: None,
        memory_thp: None,
        boot_source_id: None,
        root_disk_object_id: None,
        boot_mode: None,
        description: None,
        image_ref: None,
        cloud_init_user_data: None,
        cloud_init_meta_data: None,
        cloud_init_network_config: None,
        network_id: req.network_id,
        networks: None,
        security_group_ids: None,
        accelerator_config: None,
        numa_config: None,
        persistent_upper_pool_id: None,
        placement_policy: None,
        config: serde_json::json!({ "sandbox_exec": true }),
    };

    let resolved_vm = vms::resolve_create_request(env.pool(), new_vm).await?;
    if resolved_vm.image_ref.is_some() {
        return Err(Error::UnprocessableEntity(
            "sandbox VM templates with OCI image_ref are not supported yet".into(),
        ));
    }

    Ok(resolved_vm)
}

pub(crate) async fn destroy_vm(pool: &PgPool, vm_id: Uuid) {
    use crate::model::host_gpus;

    if let Err(e) = host_gpus::deallocate_by_vm(pool, vm_id).await {
        tracing::warn!(vm_id = %vm_id, error = %e, "Failed to deallocate GPUs for sandbox VM");
    }

    let vm = match vms::get(pool, vm_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(vm_id = %vm_id, error = %e, "Failed to get sandbox VM for deletion");
            let _ = vms::delete(pool, vm_id).await;
            return;
        }
    };

    if let Some(host_id) = vm.host_id
        && let Ok(Some(host)) = hosts::get_by_id(pool, host_id).await
    {
        let client = NodeClient::new(&host.address, host.port as u16);
        if let Err(e) = client.delete_vm(vm_id).await {
            let not_found = e
                .downcast_ref::<crate::errors::Error>()
                .map(|err| matches!(err, crate::errors::Error::NotFound))
                .unwrap_or(false);
            if !not_found {
                tracing::warn!(vm_id = %vm_id, error = %e, "delete_vm on node failed (ignoring)");
            }
        }
    }

    if let Err(e) = vms::delete(pool, vm_id).await {
        tracing::error!(vm_id = %vm_id, error = %e, "Failed to delete sandbox VM from DB");
    }
}
