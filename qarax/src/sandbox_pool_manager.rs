use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    App,
    handlers::{
        sandbox::handler::{MAX_FORKS, create_forks_tx, start_seed_forks},
        vm::handler::{create_vm_internal, start_vm_internal},
    },
    model::{
        sandbox_pool_members::{self, SandboxPoolMember, SandboxPoolMemberStatus},
        sandbox_pools::{self, SandboxPool},
        sandboxes::NewSandbox,
        vms::{self, VmStatus},
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

/// The lock that keeps one sync of a pool at a time: two at once (the
/// manager's pass and the one a claim starts) would each boot the seeds the
/// pool is short of.
fn sync_lock(pool_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    LOCKS.lock().unwrap().entry(pool_id).or_default().clone()
}

async fn sync_pool(env: &App, pool: SandboxPool) {
    let lock = sync_lock(pool.id);
    let _syncing = lock.lock().await;
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

    sync_seeds(env, &pool).await;

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
    if pool.fork {
        fork_members(env, &pool, deficit).await;
    } else {
        for _ in 0..deficit {
            if let Err(e) = provision_pool_vm(env, &pool, PoolVm::Member).await {
                warn!(
                    pool_id = %pool.id,
                    vm_template_id = %pool.vm_template_id,
                    error = %e,
                    "Sandbox pool manager: failed to provision pool member"
                );
            }
        }
    }
}

/// A forking pool's seeds: VMs of its template it keeps booted, one on each
/// of `seed_hosts` hosts, which it forks its members from, and a sandbox from
/// when it has no member ready. The pool boots the seeds it is short of,
/// whatever its `min_ready`, each on a host without one, and replaces a seed
/// that failed (destroyed with the pool's other failed members) or whose VM
/// stopped running. A seed whose host is down or in maintenance is left where
/// it is, out of the count: the pool boots another on a host that's up, and
/// once the host is back, one of them is extra. It destroys a second seed on a
/// host (one migrated there, say) and those beyond `seed_hosts`, the newest
/// first: all of them, once it no longer forks.
async fn sync_seeds(env: &App, pool: &SandboxPool) {
    let seeds = match sandbox_pool_members::seeds(env.pool(), pool.id).await {
        Ok(seeds) => seeds,
        Err(e) => {
            warn!(
                pool_id = %pool.id,
                error = %e,
                "Sandbox pool manager: failed to look up the pool's seeds"
            );
            return;
        }
    };
    let wanted = if pool.fork {
        pool.seed_hosts as usize
    } else {
        0
    };
    // The hosts of the seeds the pool keeps, booting or running.
    let mut hosts = Vec::new();
    for seed in seeds {
        // A down host's node may not answer a destroy, which would leave the
        // VM running there with no row.
        if !seed.host_up {
            continue;
        }
        let gone = match seed.member.status {
            SandboxPoolMemberStatus::Provisioning => None,
            SandboxPoolMemberStatus::Ready => match seed.vm_status {
                VmStatus::Shutdown | VmStatus::Unknown | VmStatus::Created => {
                    Some("its VM stopped running")
                }
                VmStatus::Running
                | VmStatus::Pending
                | VmStatus::Paused
                | VmStatus::Migrating
                | VmStatus::Committing => None,
            },
            // Failed seeds went with the pool's other failed members, and a
            // seed being destroyed is gone already.
            SandboxPoolMemberStatus::Error | SandboxPoolMemberStatus::Destroying => continue,
        };
        let extra = if hosts.len() >= wanted {
            Some("the pool keeps fewer seeds")
        } else if seed.host_id.is_some_and(|host| hosts.contains(&host)) {
            Some("another of the pool's seeds runs on its host")
        } else {
            None
        };
        if let Some(reason) = gone {
            warn!(
                pool_id = %pool.id,
                seed_vm_id = %seed.member.vm_id,
                reason,
                "Sandbox pool manager: replacing a seed"
            );
            destroy_member(env.pool(), seed.member).await;
        } else if let Some(reason) = extra {
            info!(
                pool_id = %pool.id,
                seed_vm_id = %seed.member.vm_id,
                reason,
                "Sandbox pool manager: destroying a seed"
            );
            destroy_member(env.pool(), seed.member).await;
        } else {
            hosts.extend(seed.host_id);
        }
    }
    for _ in hosts.len()..wanted {
        match boot_seed(env, pool, &hosts).await {
            Ok(host) => hosts.extend(host),
            Err(e) => {
                warn!(
                    pool_id = %pool.id,
                    vm_template_id = %pool.vm_template_id,
                    error = %e,
                    "Sandbox pool manager: failed to boot a seed"
                );
                return;
            }
        }
    }
}

/// Boot a seed of the pool on a host without one: returns its host.
async fn boot_seed(
    env: &App,
    pool: &SandboxPool,
    other_seed_hosts: &[Uuid],
) -> Result<Option<Uuid>, crate::errors::Error> {
    let vm_id = provision_pool_vm(env, pool, PoolVm::Seed { other_seed_hosts }).await?;
    Ok(vms::get(env.pool(), vm_id).await?.host_id)
}

/// Make `count` members of a forking pool by forking its seeds, spread across
/// their hosts: each round forks the seed whose host holds the fewest of the
/// pool's members, as many as bring it level with the next (up to
/// `MAX_FORKS`). A seed that can't be forked (its host has no room, say)
/// leaves the rest to the others, and the pool tries it again on its next
/// pass.
async fn fork_members(env: &App, pool: &SandboxPool, count: usize) {
    if count == 0 {
        return;
    }
    let mut seeds =
        match sandbox_pool_members::forkable_seeds(env.pool(), pool.vm_template_id).await {
            Ok(seeds) => seeds,
            Err(e) => {
                warn!(
                    pool_id = %pool.id,
                    error = %e,
                    "Sandbox pool manager: failed to look up the seeds to fork"
                );
                return;
            }
        };
    let mut left = count;
    while left > 0 && !seeds.is_empty() {
        seeds.sort_by_key(|seed| seed.members);
        let level = seeds.get(1).map_or(i64::MAX, |next| next.members);
        let round = ((level - seeds[0].members) as usize)
            .clamp(1, MAX_FORKS as usize)
            .min(left);
        let seed_vm_id = seeds[0].vm_id;
        match fork_seed(env, pool, seed_vm_id, round).await {
            Ok(()) => {
                seeds[0].members += round as i64;
                left -= round;
            }
            Err(e) => {
                warn!(
                    pool_id = %pool.id,
                    vm_template_id = %pool.vm_template_id,
                    seed_vm_id = %seed_vm_id,
                    error = %e,
                    "Sandbox pool manager: failed to fork pool members from a seed"
                );
                seeds.remove(0);
            }
        }
    }
}

/// Make `count` members of a forking pool by forking one of its seeds from
/// the snapshot of it its node keeps: they continue from the seed's state when
/// the node took it, and are ready as soon as the node has started them.
async fn fork_seed(
    env: &App,
    pool: &SandboxPool,
    seed_vm_id: Uuid,
    count: usize,
) -> Result<(), crate::errors::Error> {
    let seed = vms::get(env.pool(), seed_vm_id).await?;
    let names: Vec<String> = (0..count).map(|_| member_vm_name(pool, false)).collect();
    let mut tx = env.pool().begin().await?;
    let forks = create_forks_tx(env, &mut tx, &seed, &names).await?;
    let mut members = Vec::with_capacity(count);
    for vm_id in &forks.vm_ids {
        members.push(sandbox_pool_members::create_tx(&mut tx, pool.id, *vm_id, false).await?);
    }
    tx.commit().await?;
    start_seed_forks(env, forks).await?;
    for member_id in members {
        sandbox_pool_members::mark_ready(env.pool(), member_id).await?;
    }
    info!(
        pool_id = %pool.id,
        seed_vm_id = %seed.id,
        count,
        "Sandbox pool manager: forked pool members from a seed"
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

/// What a VM a pool boots is to it.
#[derive(Clone, Copy)]
enum PoolVm<'a> {
    Member,
    /// A seed, kept off the hosts of the pool's other seeds.
    Seed {
        other_seed_hosts: &'a [Uuid],
    },
}

/// Boot a VM of the pool's template, a member or a seed: returns its ID.
async fn provision_pool_vm(
    env: &App,
    pool: &SandboxPool,
    pool_vm: PoolVm<'_>,
) -> Result<Uuid, crate::errors::Error> {
    let seed = matches!(pool_vm, PoolVm::Seed { .. });
    let req = NewSandbox {
        name: member_vm_name(pool, seed),
        vm_template_id: pool.vm_template_id,
        idle_timeout_secs: Some(env.sandbox().pool_member_idle_timeout_secs),
        instance_type_id: None,
        network_id: None,
    };
    let mut resolved_vm = resolve_sandbox_vm(env, &req).await?;
    if let PoolVm::Seed { other_seed_hosts } = pool_vm {
        resolved_vm.excluded_host_ids = other_seed_hosts.to_vec();
    }
    let (vm_id, image_pull_job) = if resolved_vm.image_ref.is_some() {
        let (vm_id, job_id) =
            crate::handlers::vm::handler::create_vm_with_image_internal(env, resolved_vm).await?;
        (vm_id, Some(job_id))
    } else {
        (create_vm_internal(env, resolved_vm).await?, None)
    };
    // A VM the pool has no row for would be no one's.
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
                return Ok(vm_id);
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

    Ok(vm_id)
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
