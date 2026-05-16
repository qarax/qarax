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
    },
    sandbox_runtime::{self, ReadyOutcome, destroy_vm, resolve_sandbox_vm},
};

pub async fn start_sandbox_pool_manager(env: App) {
    let mut ticker = interval(Duration::from_secs(
        env.sandbox().pool_manager_interval_secs,
    ));

    loop {
        ticker.tick().await;
        if env.maintenance_mode() {
            continue;
        }
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
        idle_timeout_secs: Some(env.sandbox().pool_member_idle_timeout_secs),
        instance_type_id: None,
        network_id: None,
    };
    let resolved_vm = resolve_sandbox_vm(env, &req).await?;
    let is_oci = resolved_vm.image_ref.is_some();

    let (vm_id, initial_job_id) = if is_oci {
        crate::handlers::vm::handler::create_vm_with_image_internal(env, resolved_vm).await?
    } else {
        let id = create_vm_internal(env, resolved_vm).await?;
        (id, Uuid::nil())
    };
    let member = sandbox_pool_members::create(env.pool(), pool.id, vm_id)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    let job_id = if is_oci {
        initial_job_id
    } else {
        match start_vm_internal(env, vm_id).await {
            Ok(job_id) => job_id,
            Err(e) => {
                let _ = sandbox_pool_members::update_status(
                    env.pool(),
                    member.id,
                    SandboxPoolMemberStatus::Error,
                    Some(e.to_string()),
                )
                .await;
                return Ok(());
            }
        }
    };

    info!(
        pool_id = %pool.id,
        member_id = %member.id,
        vm_id = %vm_id,
        job_id = %job_id,
        oci = is_oci,
        "Sandbox pool manager: prewarming sandbox VM"
    );
    spawn_member_ready_watcher(env.clone(), member.id, vm_id, job_id, is_oci);

    Ok(())
}

fn spawn_member_ready_watcher(env: App, member_id: Uuid, vm_id: Uuid, job_id: Uuid, is_oci: bool) {
    tokio::spawn(async move {
        let outcome = sandbox_runtime::watch_for_ready(&env, vm_id, job_id, is_oci).await;
        match outcome {
            ReadyOutcome::Ready => {
                let _ = sandbox_pool_members::update_status(
                    env.pool(),
                    member_id,
                    SandboxPoolMemberStatus::Ready,
                    None,
                )
                .await;
                info!(member_id = %member_id, vm_id = %vm_id, "Sandbox pool member ready");
            }
            ReadyOutcome::Failed(msg) => {
                warn!(
                    member_id = %member_id,
                    vm_id = %vm_id,
                    error = %msg,
                    "Sandbox pool member failed"
                );
                let _ = sandbox_pool_members::update_status(
                    env.pool(),
                    member_id,
                    SandboxPoolMemberStatus::Error,
                    Some(msg),
                )
                .await;
            }
            ReadyOutcome::TimedOut => {
                let _ = sandbox_pool_members::update_status(
                    env.pool(),
                    member_id,
                    SandboxPoolMemberStatus::Error,
                    Some("timed out waiting for VM to start".to_string()),
                )
                .await;
            }
            ReadyOutcome::Gone => {}
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
