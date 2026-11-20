use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    App,
    handlers::{
        sandbox::handler::{MAX_FORKS, create_forks_tx, start_forks},
        vm::handler::{create_vm_internal, start_vm_internal},
    },
    model::{
        sandbox_pool_members::{self, SandboxPoolMember, SandboxPoolMemberStatus},
        sandbox_pools::{self, SandboxPool},
        sandboxes::NewSandbox,
        vms::{self, Vm, VmStatus},
    },
    pagination::PageRequest,
    sandbox_runtime::{self, ReadyOutcome, ReadyWait, destroy_vm, resolve_sandbox_vm},
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
    let pools = sandbox_pools::list(env.pool(), &PageRequest::all()).await?;
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

    let seed = sync_seed(env, &pool).await;

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

    let deficit = (pool.min_ready - (ready_count + provisioning_count) as i32).max(0) as usize;
    match (pool.fork, seed) {
        (false, _) => {
            for _ in 0..deficit {
                if let Err(e) = provision_pool_member(env, &pool, false).await {
                    warn!(
                        pool_id = %pool.id,
                        vm_template_id = %pool.vm_template_id,
                        error = %e,
                        "Sandbox pool manager: failed to provision pool member"
                    );
                }
            }
        }
        (true, Some(seed)) => {
            let mut left = deficit;
            while left > 0 {
                let count = left.min(MAX_FORKS as usize);
                if let Err(e) = fork_pool_members(env, &pool, &seed, count).await {
                    warn!(
                        pool_id = %pool.id,
                        vm_template_id = %pool.vm_template_id,
                        seed_vm_id = %seed.id,
                        error = %e,
                        "Sandbox pool manager: failed to fork pool members from the seed"
                    );
                    return;
                }
                left -= count;
            }
        }
        // The pool makes its members once its seed runs.
        (true, None) => {}
    }
}

/// A forking pool's seed, the VM of its template it keeps booted and forks
/// its members from: returned once it runs. The pool boots a seed when it has
/// none, and replaces one that failed (destroyed with the pool's other failed
/// members) or whose VM stopped running. A pool that doesn't fork, or keeps
/// no members, has no seed.
async fn sync_seed(env: &App, pool: &SandboxPool) -> Option<Vm> {
    let seed = match sandbox_pool_members::seed(env.pool(), pool.id).await {
        Ok(seed) => seed,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                error = %e,
                "Sandbox pool manager: failed to look up the pool's seed"
            );
            return None;
        }
    };
    let wanted = pool.fork && pool.min_ready > 0;
    let seed = match (seed, wanted) {
        (None, false) => return None,
        (None, true) => {
            if let Err(e) = provision_pool_member(env, pool, true).await {
                warn!(
                    pool_id = %pool.id,
                    vm_template_id = %pool.vm_template_id,
                    error = %e,
                    "Sandbox pool manager: failed to boot the pool's seed"
                );
            }
            return None;
        }
        (Some(seed), false) => {
            destroy_member(env.pool(), seed).await;
            return None;
        }
        (Some(seed), true) => seed,
    };
    // Booting, or being destroyed.
    if seed.status != SandboxPoolMemberStatus::Ready {
        return None;
    }
    let vm = match vms::get(env.pool(), seed.vm_id).await {
        Ok(vm) => vm,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                seed_vm_id = %seed.vm_id,
                error = %e,
                "Sandbox pool manager: failed to look up the seed's VM"
            );
            return None;
        }
    };
    match vm.status {
        VmStatus::Running => Some(vm),
        VmStatus::Shutdown | VmStatus::Unknown | VmStatus::Created => {
            warn!(
                pool_id = %pool.id,
                seed_vm_id = %vm.id,
                status = %vm.status,
                "Sandbox pool manager: the pool's seed stopped running, replacing it"
            );
            destroy_member(env.pool(), seed).await;
            None
        }
        VmStatus::Pending | VmStatus::Paused | VmStatus::Migrating | VmStatus::Committing => None,
    }
}

/// Make `count` members of a forking pool by forking its seed: they are ready
/// as soon as the node has started them.
async fn fork_pool_members(
    env: &App,
    pool: &SandboxPool,
    seed: &Vm,
    count: usize,
) -> Result<(), crate::errors::Error> {
    let names: Vec<String> = (0..count).map(|_| member_vm_name(pool, false)).collect();
    let mut tx = env.pool().begin().await?;
    let forks = create_forks_tx(env, &mut tx, seed, &names).await?;
    let mut members = Vec::with_capacity(count);
    for vm_id in &forks.vm_ids {
        members.push(sandbox_pool_members::create_tx(&mut tx, pool.id, *vm_id, false).await?);
    }
    tx.commit().await?;
    start_forks(env, forks).await?;
    for member_id in members {
        sandbox_pool_members::mark_ready(env.pool(), member_id).await?;
    }
    info!(
        pool_id = %pool.id,
        seed_vm_id = %seed.id,
        count,
        "Sandbox pool manager: forked pool members from the seed"
    );
    Ok(())
}

/// `sandbox-pool-<template>-<8 hex>`, `sandbox-pool-<template>-seed-<8 hex>`
/// for a seed.
fn member_vm_name(pool: &SandboxPool, seed: bool) -> String {
    format!(
        "sandbox-pool-{}{}-{}",
        &pool.vm_template_id.to_string()[..8],
        if seed { "-seed" } else { "" },
        &Uuid::new_v4().to_string()[..8]
    )
}

/// Boot a member of the pool from its template, or its seed.
async fn provision_pool_member(
    env: &App,
    pool: &SandboxPool,
    seed: bool,
) -> Result<(), crate::errors::Error> {
    let req = NewSandbox {
        name: member_vm_name(pool, seed),
        vm_template_id: pool.vm_template_id,
        idle_timeout_secs: Some(env.sandbox().pool_member_idle_timeout_secs),
        instance_type_id: None,
        network_id: None,
    };
    let resolved_vm = resolve_sandbox_vm(env, &req).await?;
    let (vm_id, image_pull_job) = if resolved_vm.image_ref.is_some() {
        let (vm_id, job_id) =
            crate::handlers::vm::handler::create_vm_with_image_internal(env, resolved_vm).await?;
        (vm_id, Some(job_id))
    } else {
        (create_vm_internal(env, resolved_vm).await?, None)
    };
    // A second seed, booted by a sync racing this one, has no row to go in.
    let member = match sandbox_pool_members::create(env.pool(), pool.id, vm_id, seed).await {
        Ok(member) => member,
        Err(e) => {
            destroy_vm(env.pool(), vm_id).await;
            return Err(crate::errors::Error::Sqlx(e));
        }
    };

    let wait = match image_pull_job {
        Some(job_id) => ReadyWait::PullingImage { job_id },
        None => match start_vm_internal(env, vm_id).await {
            Ok(job_id) => ReadyWait::Starting { job_id },
            Err(e) => {
                let _ =
                    sandbox_pool_members::mark_error(env.pool(), member.id, &e.to_string()).await;
                return Ok(());
            }
        },
    };

    info!(
        pool_id = %pool.id,
        member_id = %member.id,
        vm_id = %vm_id,
        seed,
        wait = ?wait,
        "Sandbox pool manager: booting a pool VM"
    );
    spawn_member_ready_watcher(env.clone(), member.id, vm_id, wait);

    Ok(())
}

fn spawn_member_ready_watcher(env: App, member_id: Uuid, vm_id: Uuid, wait: ReadyWait) {
    tokio::spawn(async move {
        let outcome = sandbox_runtime::watch_for_ready(&env, vm_id, wait).await;
        let error = match outcome {
            ReadyOutcome::Ready => {
                let _ = sandbox_pool_members::mark_ready(env.pool(), member_id).await;
                info!(member_id = %member_id, vm_id = %vm_id, "Sandbox pool member ready");
                return;
            }
            ReadyOutcome::Failed(msg) => msg,
            ReadyOutcome::TimedOut => "timed out waiting for VM to start".to_string(),
            ReadyOutcome::Gone => return,
        };
        warn!(
            member_id = %member_id,
            vm_id = %vm_id,
            error = %error,
            "Sandbox pool member failed"
        );
        let _ = sandbox_pool_members::mark_error(env.pool(), member_id, &error).await;
    });
}

pub(crate) async fn destroy_member(pool: &PgPool, member: SandboxPoolMember) {
    match sandbox_pool_members::mark_destroying(pool, member.id).await {
        Ok(true) => {}
        // Claimed by a sandbox since it was listed, or already being
        // destroyed: its VM is not ours to destroy.
        Ok(false) => return,
        Err(e) => {
            warn!(
                member_id = %member.id,
                vm_id = %member.vm_id,
                error = %e,
                "Sandbox pool manager: failed to mark member destroying"
            );
            return;
        }
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
