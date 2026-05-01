use std::{path::Path as StdPath, time::Duration};

use axum::{Extension, Json, extract::Path};
use http::StatusCode;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, PgConnection};
use tokio::{fs, process::Command, time::sleep};
use tracing::{error, instrument};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    App,
    handlers::{
        audit::{AuditEvent, AuditEventExt},
        vm::handler::{CreateSnapshotRequest, create_vm_snapshot, restore_vm_from_snapshot},
    },
    model::{
        audit_log::{AuditAction, AuditResourceType},
        backups::{self, Backup, BackupStatus, BackupType, NewBackup},
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools,
    },
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

struct MaintenanceModeGuard(App);

impl MaintenanceModeGuard {
    fn new(env: App) -> Self {
        env.set_maintenance_mode(true);
        Self(env)
    }
}

impl Drop for MaintenanceModeGuard {
    fn drop(&mut self) {
        self.0.set_maintenance_mode(false);
    }
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
            status: BackupStatus::Ready,
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

    sqlx::query(
        r#"
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = $1
  AND pid <> pg_backend_pid()
        "#,
    )
    .bind(&env.database().name)
    .execute(&mut connection)
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
            status: BackupStatus::Ready,
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
    path = "/backups",
    params(crate::handlers::BackupListQuery),
    responses(
        (status = 200, description = "List backups", body = Vec<Backup>),
        (status = 500, description = "Internal server error")
    ),
    tag = "backups"
)]
#[instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<App>,
    axum::extract::Query(query): axum::extract::Query<crate::handlers::BackupListQuery>,
) -> Result<ApiResponse<Vec<Backup>>> {
    let backups = backups::list(env.pool(), query.name.as_deref(), query.backup_type).await?;
    Ok(ApiResponse {
        data: backups,
        code: StatusCode::OK,
    })
}

#[utoipa::path(
    get,
    path = "/backups/{backup_id}",
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
    path = "/backups",
    request_body = CreateBackupRequest,
    responses(
        (status = 201, description = "Backup created", body = Backup),
        (status = 404, description = "VM not found"),
        (status = 422, description = "Invalid backup request"),
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
    path = "/backups/{backup_id}/restore",
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

            let _guard = MaintenanceModeGuard::new(env.clone());
            sleep(Duration::from_millis(500)).await;
            terminate_database_sessions(&env).await?;
            run_pg_restore(&env, &dump_path).await?;

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
