use std::sync::Arc;

use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    App,
    handlers::vm::handler::{create_vm_internal, start_vm_internal},
    model::{
        sandbox_pool_members::{self, SandboxPoolMember, SandboxPoolMemberStatus},
        sandbox_pools::{self, SandboxPool},
        sandboxes::NewSandbox,
        vms::{self, VmStatus},
    },
    sandbox_runtime::{destroy_vm, resolve_sandbox_vm},
};

pub async fn start_sandbox_pool_manager(env: App) {
    let mut ticker = interval(Duration::from_secs(10));

    loop {
        ticker.tick().await;
        if let Err(e) = sync_all_pools(&env).await {
            warn!("Sandbox pool manager: failed to sync pools: {}", e);
        }
    }
}

async fn sync_all_pools(env: &App) -> Result<(), sqlx::Error> {
    let pools = sandbox_pools::list(env.pool()).await?;
    for pool in pools {
        sync_pool(env, pool).await;
    }
    Ok(())
}

pub(crate) async fn sync_pool_for_template(
    env: &App,
    vm_template_id: Uuid,
) -> Result<(), sqlx::Error> {
    let pool = sandbox_pools::get_by_template(env.pool(), vm_template_id).await?;
    sync_pool(env, pool).await;
    Ok(())
}

async fn sync_pool(env: &App, pool: SandboxPool) {
    let error_members = match sandbox_pool_members::list_error_by_pool(env.pool(), pool.id).await {
        Ok(members) => members,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                vm_template_id = %pool.vm_template_id,
                error = %e,
                "Sandbox pool manager: failed to list error members"
            );
            return;
        }
    };

    for member in error_members {
        destroy_member(env.pool(), member).await;
    }

    let ready_members = match sandbox_pool_members::list_ready_by_pool(env.pool(), pool.id).await {
        Ok(members) => members,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                vm_template_id = %pool.vm_template_id,
                error = %e,
                "Sandbox pool manager: failed to list ready members"
            );
            return;
        }
    };

    let surplus = ready_members.len() as i32 - pool.min_ready;
    if surplus > 0 {
        for member in ready_members.into_iter().take(surplus as usize) {
            destroy_member(env.pool(), member).await;
        }
    }

    let ready_count = match sandbox_pool_members::count_by_status(
        env.pool(),
        pool.id,
        SandboxPoolMemberStatus::Ready,
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                vm_template_id = %pool.vm_template_id,
                error = %e,
                "Sandbox pool manager: failed to count ready members"
            );
            return;
        }
    };
    let provisioning_count = match sandbox_pool_members::count_by_status(
        env.pool(),
        pool.id,
        SandboxPoolMemberStatus::Provisioning,
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                vm_template_id = %pool.vm_template_id,
                error = %e,
                "Sandbox pool manager: failed to count provisioning members"
            );
            return;
        }
    };

    let deficit = pool.min_ready - (ready_count + provisioning_count) as i32;
    for _ in 0..deficit.max(0) {
        if let Err(e) = provision_pool_member(env, &pool).await {
            warn!(
                pool_id = %pool.id,
                vm_template_id = %pool.vm_template_id,
                error = %e,
                "Sandbox pool manager: failed to provision pool member"
            );
        }
    }
}

async fn provision_pool_member(env: &App, pool: &SandboxPool) -> Result<(), crate::errors::Error> {
    let internal_name = format!(
        "sandbox-pool-{}-{}",
        &pool.vm_template_id.to_string()[..8],
        &Uuid::new_v4().to_string()[..8]
    );
    let req = NewSandbox {
        name: internal_name.clone(),
        vm_template_id: pool.vm_template_id,
        idle_timeout_secs: Some(300),
        instance_type_id: None,
        network_id: None,
    };
    let resolved_vm = resolve_sandbox_vm(env, &req).await?;
    let vm_id = create_vm_internal(env, resolved_vm).await?;
    let member = sandbox_pool_members::create(env.pool(), pool.id, vm_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    match start_vm_internal(env, vm_id).await {
        Ok(job_id) => {
            info!(
                pool_id = %pool.id,
                member_id = %member.id,
                vm_id = %vm_id,
                job_id = %job_id,
                "Sandbox pool manager: prewarming sandbox VM"
            );
            spawn_member_ready_watcher(env.pool_arc(), member.id, vm_id);
        }
        Err(e) => {
            let _ = sandbox_pool_members::update_status(
                env.pool(),
                member.id,
                SandboxPoolMemberStatus::Error,
                Some(e.to_string()),
            )
            .await;
        }
    }

    Ok(())
}

fn spawn_member_ready_watcher(pool: Arc<PgPool>, member_id: Uuid, vm_id: Uuid) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(2));
        let mut attempts = 0u32;
        loop {
            ticker.tick().await;
            attempts += 1;
            if attempts > 150 {
                let _ = sandbox_pool_members::update_status(
                    &pool,
                    member_id,
                    SandboxPoolMemberStatus::Error,
                    Some("timed out waiting for VM to start".to_string()),
                )
                .await;
                break;
            }

            match vms::get(&pool, vm_id).await {
                Ok(vm) => match vm.status {
                    VmStatus::Running => {
                        let _ = sandbox_pool_members::update_status(
                            &pool,
                            member_id,
                            SandboxPoolMemberStatus::Ready,
                            None,
                        )
                        .await;
                        break;
                    }
                    VmStatus::Created | VmStatus::Shutdown | VmStatus::Unknown => {
                        let _ = sandbox_pool_members::update_status(
                            &pool,
                            member_id,
                            SandboxPoolMemberStatus::Error,
                            Some("VM failed to start".to_string()),
                        )
                        .await;
                        break;
                    }
                    _ => continue,
                },
                Err(sqlx::Error::RowNotFound) => break,
                Err(e) => {
                    warn!(
                        member_id = %member_id,
                        vm_id = %vm_id,
                        error = %e,
                        "Sandbox pool manager: failed to poll VM status"
                    );
                }
            }
        }
    });
}

pub(crate) async fn destroy_member(pool: &PgPool, member: SandboxPoolMember) {
    if let Err(e) = sandbox_pool_members::update_status(
        pool,
        member.id,
        SandboxPoolMemberStatus::Destroying,
        member.error_message.clone(),
    )
    .await
    {
        warn!(
            member_id = %member.id,
            vm_id = %member.vm_id,
            error = %e,
            "Sandbox pool manager: failed to mark member destroying"
        );
    }

    destroy_vm(pool, member.vm_id).await;

    if let Err(e) = sandbox_pool_members::delete(pool, member.id).await {
        warn!(
            member_id = %member.id,
            vm_id = %member.vm_id,
            error = %e,
            "Sandbox pool manager: failed to delete member row"
        );
    }
}
