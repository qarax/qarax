use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use sqlx::{
    types::{ipnetwork::IpNetwork, mac_address::MacAddress},
    PgPool, Type,
};
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
    pub mac_address: Option<Vec<u8>>,
    pub ip_address: IpAddr,
    pub network_mode: NetworkMode,
    pub kernel_params: String,
    pub kernel: Uuid,
}

#[derive(sqlx::FromRow)]
pub struct VmRow {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub status: VmStatus,
    pub vcpu: i32,
    pub memory: i32,
    pub mac_address: Option<MacAddress>,
    pub ip_address: Option<IpNetwork>,
    pub network_mode: NetworkMode,
    pub kernel_params: String,
    pub kernel: Uuid,
}

impl From<VmRow> for Vm {
    fn from(vm: VmRow) -> Self {
        let mac = vm.mac_address.map(|mac| mac.bytes().to_vec());
        Vm {
            id: vm.id,
            name: vm.name,
            status: vm.status,
            host_id: vm.host_id,
            vcpu: vm.vcpu,
            memory: vm.memory,
            ip_address: vm.ip_address.unwrap().ip(),
            mac_address: mac,
            network_mode: vm.network_mode,
            kernel_params: vm.kernel_params,
            kernel: vm.kernel,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "network_mode")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[derive(Default)]
pub enum NetworkMode {
    Static,
    Dhcp,
    #[default]
    None,
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
    pub kernel: Uuid,

    #[serde(default)]
    pub network_mode: NetworkMode,

    pub ip_address: Option<String>,
    pub mac_address: Option<String>,
    pub kernel_params: Option<String>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<Vm>, sqlx::Error> {
    let vms = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        vcpu,
        memory,
        ip_address as "ip_address?", 
        mac_address as "mac_address?", 
        network_mode as "network_mode: _", 
        kernel_params, 
        kernel
FROM vms
        "#
    )
    .fetch_all(pool)
    .await?;

    let vms = vms.into_iter().map(|vm| vm.into()).collect();
    Ok(vms)
}

pub async fn get(pool: &PgPool, vm_id: Uuid) -> Result<Vm, sqlx::Error> {
    let vm = sqlx::query_as!(
        VmRow,
        r#"
SELECT id,
        name,
        status as "status: _",
        host_id as "host_id?",
        vcpu,
        memory,
        ip_address as "ip_address?", 
        mac_address as "mac_address?", 
        network_mode as "network_mode: _", 
        kernel_params, 
        kernel
FROM vms
WHERE id = $1
        "#,
        vm_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm.into())
}
