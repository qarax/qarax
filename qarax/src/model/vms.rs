use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type, types::Json};
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub vcpu: i32,
    pub memory: i32,
    pub hypervisor: Hypervisor,
    pub config: serde_json::Value,
    pub boot_source_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(sqlx::FromRow)]
pub struct VmRow {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub vcpu: i32,
    pub memory: i32,
    pub hypervisor: Hypervisor,
    pub config: Json<serde_json::Value>,
    pub boot_source_id: Option<Uuid>,
    pub description: Option<String>,
}

impl From<VmRow> for Vm {
    fn from(row: VmRow) -> Self {
        Vm {
            id: row.id,
            name: row.name,
            status: row.status,
            host_id: row.host_id,
            vcpu: row.vcpu,
            memory: row.memory,
            hypervisor: row.hypervisor,
            config: row.config.0,
            boot_source_id: row.boot_source_id,
            description: row.description,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "hypervisor")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Hypervisor {
    CloudHv,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "vm_status")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum VmStatus {
    Up,
    Down,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewVm {
    pub name: String,
    pub vcpu: i32,
    pub memory: i32,
    pub hypervisor: Hypervisor,

    #[serde(default)]
    pub config: serde_json::Value,

    pub boot_source_id: Option<Uuid>,
    pub description: Option<String>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<Vm>, sqlx::Error> {
    let vms: Vec<VmRow> = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        vcpu,
        memory,
        hypervisor as "hypervisor: _",
        config as "config: _",
        boot_source_id as "boot_source_id?",
        description as "description?"
FROM vms
        "#
    )
    .fetch_all(pool)
    .await?;

    let vms: Vec<Vm> = vms.into_iter().map(|vm: VmRow| vm.into()).collect();
    Ok(vms)
}

pub async fn get(pool: &PgPool, vm_id: Uuid) -> Result<Vm, sqlx::Error> {
    let vm: VmRow = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        vcpu,
        memory,
        hypervisor as "hypervisor: _",
        config as "config: _",
        boot_source_id as "boot_source_id?",
        description as "description?"
FROM vms
WHERE id = $1
        "#,
        vm_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm.into())
}
