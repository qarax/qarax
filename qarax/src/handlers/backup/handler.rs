use std::{mem::DropGuard, path::Path as StdPath, sync::Arc, time::Duration};

use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, PgConnection, PgPool};
use tokio::{fs, process::Command, time::sleep};
use tracing::{error, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    App,
    grpc_client::NodeClient,
    handlers::{
        audit::{AuditEvent, AuditEventExt},
        vm::handler::{
            CreateSnapshotRequest, create_vm_snapshot, delete_snapshot_files,
            restore_vm_from_snapshot,
        },
    },
    model::{
        audit_log::{AuditAction, AuditResourceType},
        backups::{self, Backup, BackupStatus, BackupType, NewBackup},
        hosts::{self, Host, HostStatus},
        jobs::{self, JobType, NewJob},
        snapshots::{self, NewSnapshot, SnapshotStatus},
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools::{self, StoragePoolStatus, StoragePoolType},
        vms,
    },
    pagination::{PageQuery, PageRequest, time_key},
};

use super::{ApiResponse, Result};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateBackupRequest {
    /// Human-readable backup name. Defaults to a generated name when omitted.
    pub name: Option<String>,
    /// Preferred storage pool for the backup artifact.
    pub storage_pool_id: Option<Uuid>,
    /// Backup type to create.
    pub backup_type: BackupType,
    /// VM to back up when `backup_type=vm`.
    pub vm_id: Option<Uuid>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreBackupResponse {
    pub backup_id: Uuid,
    pub backup_type: BackupType,
    pub vm_id: Option<Uuid>,
    pub database_name: Option<String>,
}

fn database_backup_name(name: Option<&str>) -> String {
    name.map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("database-backup-{}", &Uuid::new_v4().to_string()[..8]))
}

async fn record_ready_vm_backup(
    pool: &sqlx::PgPool,
    snapshot: &crate::model::snapshots::Snapshot,
) -> Result<Backup> {
    backups::create(
        pool,
        &NewBackup {
            name: snapshot.name.clone(),
            backup_type: BackupType::Vm,
            vm_id: Some(snapshot.vm_id),
            snapshot_id: Some(snapshot.id),
            storage_object_id: snapshot.storage_object_id,
        },
    )
    .await
    .map_err(Into::into)
}

async fn resolve_database_backup_pool(env: &App, preferred_pool_id: Option<Uuid>) -> Result<Uuid> {
    storage_pools::pick_active_local_pool(env.pool(), preferred_pool_id)
        .await?
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "no suitable local storage pool available for database backup".into(),
            )
        })
}

fn configure_pg_command(command: &mut Command, env: &App) {
    let database = env.database();
    command
        .env("PGPASSWORD", database.password.expose_secret())
        .arg("--host")
        .arg(&database.host)
        .arg("--port")
        .arg(database.port.to_string())
        .arg("--username")
        .arg(&database.username)
        .arg("--dbname")
        .arg(&database.name);
}

async fn run_pg_dump(env: &App, dump_path: &str) -> Result<i64> {
    if let Some(parent) = StdPath::new(dump_path).parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|_| crate::errors::Error::InternalServerError)?;
    }

    let mut command = Command::new("pg_dump");
    configure_pg_command(&mut command, env);
    let output = command
        .arg("--format=custom")
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg("--file")
        .arg(dump_path)
        .output()
        .await
        .map_err(|e| {
            error!(error = %e, "failed to spawn pg_dump");
            crate::errors::Error::InternalServerError
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "pg_dump failed");
        return Err(crate::errors::Error::InternalServerError);
    }

    let metadata = fs::metadata(dump_path)
        .await
        .map_err(|_| crate::errors::Error::InternalServerError)?;
    Ok(metadata.len() as i64)
}

async fn cleanup_dump_file(dump_path: &str) {
    let _ = fs::remove_file(dump_path).await;
}

async fn terminate_database_sessions(env: &App) -> Result<()> {
    let maintenance_url = format!("{}/postgres", env.database().connection_string_without_db());
    let mut connection = PgConnection::connect(&maintenance_url)
        .await
        .map_err(crate::errors::Error::Sqlx)?;

    sqlx::query!(
        r#"
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = $1
  AND pid <> pg_backend_pid()
        "#,
        &env.database().name
    )
    .fetch_all(&mut connection)
    .await
    .map_err(crate::errors::Error::Sqlx)?;

    Ok(())
}

async fn run_pg_restore(env: &App, dump_path: &str) -> Result<()> {
    let mut command = Command::new("pg_restore");
    configure_pg_command(&mut command, env);
    let output = command
        .arg("--clean")
        .arg("--if-exists")
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg("--single-transaction")
        .arg("--exit-on-error")
        .arg(dump_path)
        .output()
        .await
        .map_err(|e| {
            error!(error = %e, "failed to spawn pg_restore");
            crate::errors::Error::InternalServerError
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "pg_restore failed");
        return Err(crate::errors::Error::InternalServerError);
    }

    Ok(())
}

async fn create_database_backup(env: &App, request: &CreateBackupRequest) -> Result<Backup> {
    let pool_id = resolve_database_backup_pool(env, request.storage_pool_id).await?;
    let name = database_backup_name(request.name.as_deref());
    let storage_pool = storage_pools::get(env.pool(), pool_id).await?;
    let base_path = storage_pool
        .config
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            crate::errors::Error::UnprocessableEntity(
                "selected storage pool is missing a writable local path".into(),
            )
        })?;
    let dump_path = format!("{}/{}.dump", base_path, Uuid::new_v4());

    let size_bytes = match run_pg_dump(env, &dump_path).await {
        Ok(size_bytes) => size_bytes,
        Err(error) => {
            cleanup_dump_file(&dump_path).await;
            return Err(error);
        }
    };

    let storage_object = match storage_objects::create_returning(
        env.pool(),
        NewStorageObject {
            name: name.clone(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::DatabaseBackup,
            size_bytes,
            config: serde_json::json!({ "path": dump_path }),
            parent_id: None,
        },
    )
    .await
    {
        Ok(storage_object) => storage_object,
        Err(error) => {
            cleanup_dump_file(&dump_path).await;
            return Err(error.into());
        }
    };

    backups::create(
        env.pool(),
        &NewBackup {
            name,
            backup_type: BackupType::Database,
            vm_id: None,
            snapshot_id: None,
            storage_object_id: storage_object.id,
        },
    )
    .await
    .map_err(|error| {
        error!(error = %error, storage_object_id = %storage_object.id, "failed to persist database backup");
        error
    })
    .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/v1/backups",
    params(crate::handlers::BackupListQuery, PageQuery),
    responses(
        (status = 200, description = "Backups, newest first", body = crate::pagination::BackupPage),
        (status = 422, description = "Invalid limit or cursor"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::BackupListQuery>,
    axum::extract::Query(page): axum::extract::Query<PageQuery>,
) -> Result<ApiResponse<crate::pagination::BackupPage>> {
    let page = PageRequest::try_from(page)?;
    let backups =
        backups::list(env.pool(), query.name.as_deref(), query.backup_type, &page).await?;
    Ok(ApiResponse {
        data: crate::pagination::BackupPage::new(backups, &page, |backup| {
            (time_key(backup.created_at), backup.id)
        }),
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/v1/backups/{backup_id}",
    params(
        ("backup_id" = uuid::Uuid, Path, description = "Backup unique identifier")
    ),
    responses(
        (status = 200, description = "Backup details", body = Backup),
        (status = 404, description = "Backup not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(backup_id): Path<Uuid>,
) -> Result<ApiResponse<Backup>> {
    let backup = backups::get(env.pool(), backup_id).await?;
    Ok(ApiResponse {
        data: backup,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    post,
    path = "/v1/backups",
    request_body = CreateBackupRequest,
    responses(
        (status = 201, description = "Backup created", body = Backup),
        (status = 404, description = "VM not found"),
        (status = 422, description = "Invalid backup request, or a VM with a writable disk a backup can't capture"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn create(
    Extension(env): Extension<App>,
    Json(body): Json<CreateBackupRequest>,
) -> Result<axum::response::Response> {
    let backup = match body.backup_type.clone() {
        BackupType::Vm => {
            let vm_id = body.vm_id.ok_or_else(|| {
                crate::errors::Error::UnprocessableEntity(
                    "vm_id is required when backup_type is vm".into(),
                )
            })?;
            let snapshot = create_vm_snapshot(
                &env,
                vm_id,
                &CreateSnapshotRequest {
                    name: body.name.clone(),
                    storage_pool_id: body.storage_pool_id,
                },
            )
            .await?;
            record_ready_vm_backup(env.pool(), &snapshot).await?
        }
        BackupType::Database => {
            if body.vm_id.is_some() {
                return Err(crate::errors::Error::UnprocessableEntity(
                    "vm_id is not supported when backup_type is database".into(),
                ));
            }
            create_database_backup(&env, &body).await?
        }
    };

    Ok(ApiResponse {
        data: backup.clone(),
        code: StatusCode::CREATED,
    }
    .with_audit_event(AuditEvent {
        action: AuditAction::Create,
        resource_type: AuditResourceType::Backup,
        resource_id: backup.id,
        resource_name: Some(backup.name),
        metadata: None,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/backups/{backup_id}/restore",
    params(
        ("backup_id" = uuid::Uuid, Path, description = "Backup unique identifier")
    ),
    responses(
        (status = 200, description = "Backup restored", body = RestoreBackupResponse),
        (status = 404, description = "Backup not found"),
        (status = 422, description = "Backup cannot be restored"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn restore(
    Extension(env): Extension<App>,
    Path(backup_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let backup = backups::get(env.pool(), backup_id).await?;
    if backup.status != BackupStatus::Ready {
        return Err(crate::errors::Error::UnprocessableEntity(
            "backup is not in ready state".into(),
        ));
    }

    let response = match backup.backup_type {
        BackupType::Vm => {
            let vm_id = backup
                .vm_id
                .ok_or(crate::errors::Error::InternalServerError)?;
            let snapshot_id = backup
                .snapshot_id
                .ok_or(crate::errors::Error::InternalServerError)?;
            let _vm = restore_vm_from_snapshot(&env, vm_id, snapshot_id).await?;
            RestoreBackupResponse {
                backup_id: backup.id,
                backup_type: backup.backup_type.clone(),
                vm_id: Some(vm_id),
                database_name: None,
            }
        }
        BackupType::Database => {
            let storage_object = storage_objects::get(env.pool(), backup.storage_object_id).await?;
            let dump_path = storage_objects::get_path_from_config(&storage_object.config)
                .ok_or(crate::errors::Error::InternalServerError)?;

            // Database backups are files outside the database, which the
            // restore must not rewind: the dump predates its own record and
            // those of later backups, and has records of backups deleted
            // since. Keep the records as they are now.
            let database_backups = backups::list_database_backup_records(env.pool()).await?;

            env.set_maintenance_mode(true);
            let _maintenance = DropGuard::new(env.clone(), |env| env.set_maintenance_mode(false));
            sleep(Duration::from_millis(500)).await;
            terminate_database_sessions(&env).await?;
            run_pg_restore(&env, &dump_path).await?;

            // The database is restored either way: a record that can't be put
            // back is logged with its dump, not turned into a failed restore.
            for (record, e) in
                backups::reset_database_backup_records(env.pool(), &database_backups).await?
            {
                warn!(
                    backup_id = %record.id,
                    dump = ?storage_objects::get_path_from_config(&record.object_config),
                    error = %e,
                    "could not re-record a database backup after the restore; its dump is untracked"
                );
            }

            RestoreBackupResponse {
                backup_id: backup.id,
                backup_type: backup.backup_type.clone(),
                vm_id: None,
                database_name: Some(env.database().name.clone()),
            }
        }
    };

    Ok(ApiResponse {
        data: response,
        code: StatusCode::OK,
    }
    .with_audit_event(AuditEvent {
        action: AuditAction::Restore,
        resource_type: AuditResourceType::Backup,
        resource_id: backup.id,
        resource_name: Some(backup.name),
        metadata: None,
    }))
}

#[utoipa::path(
    delete,
    path = "/v1/backups/{backup_id}",
    params(
        ("backup_id" = uuid::Uuid, Path, description = "Backup unique identifier")
    ),
    responses(
        (status = 204, description = "Backup and its stored data deleted"),
        (status = 404, description = "Backup not found"),
        (status = 409, description = "Backup is still being created"),
        (status = 422, description = "No UP host can reach the VM snapshot"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn delete(
    Extension(env): Extension<App>,
    Path(backup_id): Path<Uuid>,
) -> Result<axum::response::Response> {
    let backup = backups::get(env.pool(), backup_id).await?;
    if backup.status == BackupStatus::Creating {
        return Err(crate::errors::Error::Conflict(
            "backup is still being created".into(),
        ));
    }

    // Remove the stored data first, so a failure leaves the record in place
    // and the delete can be retried.
    let storage_object = storage_objects::get(env.pool(), backup.storage_object_id).await?;
    if let Some(path) = storage_objects::get_path_from_config(&storage_object.config) {
        match backup.backup_type {
            BackupType::Vm => {
                let vm_host = match backup.vm_id {
                    Some(vm_id) => vms::get(env.pool(), vm_id).await?.host_id,
                    None => None,
                };
                if !delete_snapshot_files(&env, &storage_object, vm_host).await? {
                    return Err(crate::errors::Error::UnprocessableEntity(
                        "no UP host can reach the backup's snapshot".into(),
                    ));
                }
            }
            // Database dumps are written by the control plane itself.
            BackupType::Database => match fs::remove_file(&path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    error!(error = %e, path = %path, "failed to delete database backup dump");
                    return Err(crate::errors::Error::InternalServerError);
                }
            },
        }
    }

    // Cascades to the backup row (and the VM snapshot row for VM backups).
    storage_objects::delete(env.pool(), storage_object.id).await?;

    Ok(ApiResponse {
        data: (),
        code: StatusCode::NO_CONTENT,
    }
    .with_audit_event(AuditEvent {
        action: AuditAction::Delete,
        resource_type: AuditResourceType::Backup,
        resource_id: backup.id,
        resource_name: Some(backup.name),
        metadata: None,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CopyBackupRequest {
    /// The storage pool to copy the backup to: a Local or NFS pool other than
    /// the backup's, attached to an UP host.
    pub storage_pool_id: Uuid,
    /// The copy's name. Defaults to the backup's, with "-copy" after it.
    pub name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CopyBackupResponse {
    /// The job that copies the backup. Once it completes, its result holds
    /// the copy's `backup_id`.
    pub job_id: Uuid,
}

/// Copy a VM backup to another storage pool, typically one on another host,
/// so that it outlives the host it was taken on. The node that has the
/// backup offers its files over NBD, with TLS between the nodes' certificates
/// from the node CA, and a node attached to the target pool copies them,
/// their holes left as holes. The copy is a backup of its own, of the same
/// VM, which can be restored from it on a host attached to its pool.
#[utoipa::path(
    post,
    path = "/v1/backups/{backup_id}/copy",
    params(
        ("backup_id" = uuid::Uuid, Path, description = "Backup unique identifier")
    ),
    request_body = CopyBackupRequest,
    responses(
        (status = 202, description = "Copy job accepted", body = CopyBackupResponse),
        (status = 404, description = "Backup or storage pool not found"),
        (status = 422, description = "Not a ready VM backup, or a storage pool it can't be copied to"),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn copy(
    Extension(env): Extension<App>,
    Path(backup_id): Path<Uuid>,
    Json(req): Json<CopyBackupRequest>,
) -> Result<ApiResponse<CopyBackupResponse>> {
    let refuse = |why: &str| crate::errors::Error::UnprocessableEntity(why.into());
    let backup = backups::get(env.pool(), backup_id).await?;
    if backup.backup_type != BackupType::Vm {
        return Err(refuse("only VM backups are copied"));
    }
    if backup.status != BackupStatus::Ready {
        return Err(refuse("the backup isn't ready"));
    }
    // A VM backup is recorded with its VM and snapshot.
    let (Some(vm_id), Some(snapshot_id)) = (backup.vm_id, backup.snapshot_id) else {
        return Err(crate::errors::Error::InternalServerError);
    };
    let snapshot = snapshots::get(env.pool(), snapshot_id).await?;
    let source = storage_objects::get(env.pool(), snapshot.storage_object_id).await?;
    let source_path = storage_objects::get_path_from_config(&source.config)
        .ok_or(crate::errors::Error::InternalServerError)?;

    let pool = storage_pools::get(env.pool(), req.storage_pool_id).await?;
    if !matches!(
        pool.pool_type,
        StoragePoolType::Local | StoragePoolType::Nfs
    ) {
        return Err(refuse("backups are copied to Local or NFS storage pools"));
    }
    if pool.status != StoragePoolStatus::Active {
        return Err(refuse("the storage pool isn't active"));
    }
    if pool.id == source.storage_pool_id {
        return Err(refuse("the backup is in that storage pool already"));
    }

    let vm = vms::get(env.pool(), vm_id).await?;
    let sources = up_hosts_with_snapshot(&env, source.storage_pool_id, vm.host_id).await?;
    if sources.is_empty() {
        return Err(refuse("no UP host can reach the backup's snapshot"));
    }
    let destination = up_host_attached(&env, pool.id, vm.host_id)
        .await?
        .ok_or_else(|| refuse("no UP host is attached to the storage pool"))?;

    let name = req.name.unwrap_or_else(|| format!("{}-copy", backup.name));
    let object = storage_objects::create_returning(
        env.pool(),
        NewStorageObject {
            name: name.clone(),
            storage_pool_id: Some(pool.id),
            object_type: StorageObjectType::Snapshot,
            size_bytes: 0,
            config: serde_json::Value::Null,
            parent_id: None,
        },
    )
    .await?;
    let target_path = storage_objects::get_path_from_config(&object.config)
        .ok_or(crate::errors::Error::InternalServerError)?;
    let copy_snapshot_id = snapshots::create(
        env.pool(),
        &NewSnapshot {
            vm_id,
            storage_object_id: object.id,
            name: name.clone(),
        },
    )
    .await?;
    let job = jobs::create(
        env.pool(),
        NewJob {
            job_type: JobType::BackupCopy,
            description: Some(format!(
                "Copying backup {} to storage pool {}",
                backup.name, pool.name
            )),
            resource_id: Some(backup_id),
            resource_type: Some(jobs::resource_types::BACKUP.to_string()),
        },
    )
    .await;
    let job = match job {
        Ok(job) => job,
        Err(e) => {
            // Deleting the object deletes its snapshot record.
            let _ = storage_objects::delete(env.pool(), object.id).await;
            return Err(e.into());
        }
    };

    tokio::spawn(run_backup_copy(
        env.pool_arc(),
        BackupCopy {
            job_id: job.id,
            name,
            vm_id,
            snapshot_id: copy_snapshot_id,
            object_id: object.id,
            source_url: format!("file://{source_path}"),
            target_url: format!("file://{target_path}"),
            sources,
            destination,
        },
    ));

    Ok(ApiResponse {
        data: CopyBackupResponse { job_id: job.id },
        code: StatusCode::ACCEPTED,
    })
}

/// A backup copy's job: what it copies, from where to where, and the
/// records of the copy.
struct BackupCopy {
    job_id: Uuid,
    name: String,
    vm_id: Uuid,
    /// The copy's snapshot and its storage object.
    snapshot_id: Uuid,
    object_id: Uuid,
    source_url: String,
    target_url: String,
    /// The UP hosts that may have the backup's files, the likeliest first.
    sources: Vec<Host>,
    /// The UP host that writes the copy.
    destination: Host,
}

/// The UP hosts that may have a snapshot's files: the VM's, which wrote it
/// (from a local pool its host needn't be attached to), then the hosts its
/// pool is attached to.
async fn up_hosts_with_snapshot(
    env: &App,
    pool_id: Uuid,
    vm_host_id: Option<Uuid>,
) -> Result<Vec<Host>> {
    let mut host_ids: Vec<Uuid> = vm_host_id.into_iter().collect();
    for host_id in storage_pools::list_host_ids(env.pool(), pool_id).await? {
        if !host_ids.contains(&host_id) {
            host_ids.push(host_id);
        }
    }
    let mut up = Vec::new();
    for host_id in host_ids {
        let host = hosts::require_by_id(env.pool(), host_id).await?;
        if host.status == HostStatus::Up {
            up.push(host);
        }
    }
    Ok(up)
}

/// An UP host attached to `pool_id`: the VM's, if it is one.
async fn up_host_attached(
    env: &App,
    pool_id: Uuid,
    vm_host_id: Option<Uuid>,
) -> Result<Option<Host>> {
    let mut attached = Vec::new();
    for host_id in storage_pools::list_host_ids(env.pool(), pool_id).await? {
        let host = hosts::require_by_id(env.pool(), host_id).await?;
        if host.status == HostStatus::Up {
            attached.push(host);
        }
    }
    let preferred = attached.iter().position(|host| Some(host.id) == vm_host_id);
    Ok(match preferred {
        Some(index) => Some(attached.swap_remove(index)),
        None => attached.into_iter().next(),
    })
}

async fn run_backup_copy(db: Arc<PgPool>, copy: BackupCopy) {
    if let Err(e) = jobs::mark_running(&db, copy.job_id).await {
        error!(job_id = %copy.job_id, error = %e, "failed to mark a backup copy job running");
    }
    let copied = match copy_snapshot_files(&copy).await {
        Ok(bytes) => record_copy(&db, &copy, bytes)
            .await
            .map(|backup| (backup, bytes)),
        Err(e) => Err(e),
    };
    match copied {
        Ok((backup, bytes)) => {
            let result = serde_json::json!({ "backup_id": backup.id, "bytes": bytes });
            let _ = jobs::mark_completed(&db, copy.job_id, Some(result)).await;
        }
        Err(e) => {
            error!(job_id = %copy.job_id, error = %format!("{e:#}"), "backup copy failed");
            let _ = snapshots::update_status(&db, copy.snapshot_id, SnapshotStatus::Failed).await;
            let _ = jobs::mark_failed(&db, copy.job_id, &format!("{e:#}")).await;
        }
    }
}

/// Copy the backup's snapshot from the first of its candidate hosts that
/// exports it into the copy's directory. Returns the bytes of data copied.
async fn copy_snapshot_files(copy: &BackupCopy) -> anyhow::Result<u64> {
    let export_id = copy.job_id.to_string();
    let destination = NodeClient::new(&copy.destination.address, copy.destination.port as u16);
    let mut refusals = Vec::new();
    for source in &copy.sources {
        let node = NodeClient::new(&source.address, source.port as u16);
        // Only a node that has the snapshot's files exports them.
        let (port, files) = match node.export_snapshot(&export_id, &copy.source_url).await {
            Ok(export) => export,
            Err(e) => {
                refusals.push(format!("{}: {e:#}", source.name));
                continue;
            }
        };
        let pulled = destination
            .pull_snapshot(&source.address, port, &export_id, files, &copy.target_url)
            .await;
        if let Err(e) = node.unexport_snapshot(&export_id).await {
            warn!(host = %source.name, error = %format!("{e:#}"), "failed to withdraw a backup copy's exports");
        }
        return pulled;
    }
    anyhow::bail!(
        "no host exported the backup's snapshot ({})",
        refusals.join("; ")
    )
}

/// Record a finished copy: its size, its snapshot ready, and the backup it is.
async fn record_copy(db: &PgPool, copy: &BackupCopy, bytes: u64) -> anyhow::Result<Backup> {
    let mut tx = db.begin().await?;
    storage_objects::update_size_bytes_tx(&mut tx, copy.object_id, i64::try_from(bytes)?).await?;
    tx.commit().await?;
    snapshots::update_status(db, copy.snapshot_id, SnapshotStatus::Ready).await?;
    let backup = backups::create(
        db,
        &NewBackup {
            name: copy.name.clone(),
            backup_type: BackupType::Vm,
            vm_id: Some(copy.vm_id),
            snapshot_id: Some(copy.snapshot_id),
            storage_object_id: copy.object_id,
        },
    )
    .await?;
    Ok(backup)
}
