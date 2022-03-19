use crate::handlers::ServerError;

use super::*;

const DEFAUL_KERNEL_PARAMS: &str = "console=ttyS0 reboot=k panic=1 pci=off";

#[derive(Error, Debug)]
pub enum VmError {
    #[error("Inavlid name: {0}")]
    InvalidName(#[from] ValidationError),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for VmError {
    fn from(e: sqlx::Error) -> Self {
        VmError::Other(Box::new(e))
    }
}

impl From<VmError> for ServerError {
    fn from(e: VmError) -> Self {
        match e {
            VmError::InvalidName(e) => ServerError::Validation(e.to_string()),
            VmError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub status: Status,
    pub host_id: Option<Uuid>,
    pub vcpu: i32,
    pub memory: i32,
    pub ip_address: Option<String>,
    pub mac_address: Option<String>,
    pub network_mode: String,
    pub kernel_params: String,
    pub kernel: Uuid,
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

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unknown,
    Down,
    Up,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    #[serde(rename = "dhcp")]
    Dhcp,
    #[serde(rename = "static_ip")]
    StaticIp,
    #[serde(rename = "none")]
    None,
}

impl Default for NetworkMode {
    fn default() -> Self {
        NetworkMode::None
    }
}

pub async fn list(pool: &PgPool) -> Result<Vec<Vm>, VmError> {
    let vms = sqlx::query_as!(
        Vm,
        r#"
SELECT id, name, status as "status: _", host_id, vcpu, memory, ip_address, mac_address, network_mode as "network_mode: _", kernel_params, kernel
FROM vms
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(vms)
}

pub async fn by_id(pool: &PgPool, vm_id: &Uuid) -> Result<Vm, VmError> {
    let vm = sqlx::query_as!(
        Vm,
        r#"
SELECT id, name, status as "status: _", host_id, vcpu, memory, ip_address, mac_address, network_mode as "network_mode: _", kernel_params, kernel
FROM vms
WHERE id = $1
        "#,
        vm_id
    )
    .fetch_one(pool)
    .await?;

    Ok(vm)
}

pub async fn add(pool: &PgPool, vm: &NewVm) -> Result<Uuid, VmError> {
    let kernel_params = if vm.kernel_params.is_none() {
        DEFAUL_KERNEL_PARAMS
    } else {
        vm.kernel_params.as_ref().unwrap()
    };

    let rec = sqlx::query!(
        r#"
INSERT INTO vms (name, status, vcpu, memory, kernel, network_mode, ip_address, mac_address, kernel_params)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
RETURNING id
        "#,
        vm.name,
        Status::Down as Status,
        vm.vcpu,
        vm.memory,
        vm.kernel,
        vm.network_mode as NetworkMode,
        vm.ip_address,
        vm.mac_address,
        kernel_params,
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}
