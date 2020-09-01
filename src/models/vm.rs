use super::drive::AttachedDrive;
use super::*;
use crate::schema::vms;
use diesel::PgConnection;
use std::convert::From;
use std::str::FromStr;
use uuid::Uuid;

const DEFAUL_KERNEL_PARAMS: &str = "console=ttyS0 reboot=k panic=1 pci=off";

#[derive(Insertable, Identifiable, Serialize, Deserialize, Queryable, Debug, AsChangeset, Clone)]
#[table_name = "vms"]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub status: i32,
    pub host_id: Option<Uuid>, // Add belongs_to macro
    pub vcpu: i32,
    pub memory: i32,
    pub ip_address: Option<String>,
    pub mac_address: Option<String>,
    pub network_mode: String,
    pub kernel_params: String,
    pub kernel: Uuid,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug, PartialEq)]
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

impl NetworkMode {
    pub fn as_str(&self) -> String {
        match self {
            NetworkMode::Dhcp => String::from("dhcp"),
            NetworkMode::StaticIp => String::from("static_ip"),
            NetworkMode::None => String::from("none"),
        }
    }
}

impl FromStr for NetworkMode {
    type Err = Error;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "dhcp" => Ok(NetworkMode::Dhcp),
            "static_ip" => Ok(NetworkMode::StaticIp),
            "none" => Ok(NetworkMode::None),

            _ => Err(anyhow!("bad network type")),
        }
    }
}

impl Vm {
    pub fn all(conn: &PgConnection) -> Result<Vec<Vm>> {
        use crate::schema::vms::dsl::*;
        vms.load::<Vm>(conn).map_err(|e| anyhow!(e))
    }

    pub fn by_id(vm_id: Uuid, conn: &PgConnection) -> Result<Vm> {
        use crate::schema::vms::dsl::*;

        match vms.find(vm_id).first(conn) {
            Ok(v) => Ok(v),
            Err(e) => Err(ModelError::NotFound(EntityType::Vm, vm_id.into(), anyhow!(e)).into()),
        }
    }

    pub fn insert(v: &NewVm, conn: &PgConnection) -> Result<uuid::Uuid> {
        let v = Vm::from(v);

        match diesel::insert_into(vms::table).values(&v).execute(conn) {
            Ok(_) => Ok(v.id.to_owned()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update(vm: &Vm, conn: &PgConnection) -> Result<Vm> {
        match diesel::update(vm).set(vm).get_result(conn) {
            Ok(host) => Ok(host),
            Err(e) => Err(e.into()),
        }
    }

    pub fn attach_drive(vm_id: Uuid, drive_id: Uuid, conn: &PgConnection) -> Result<()> {
        use crate::schema::vm_drives_map;
        let attached_drive = AttachedDrive { vm_id, drive_id };

        match diesel::insert_into(vm_drives_map::table)
            .values(&attached_drive)
            .execute(conn)
        {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        use crate::schema::vms::dsl::*;

        diesel::delete(vms).execute(conn)
    }
}

impl From<&NewVm> for Vm {
    fn from(nv: &NewVm) -> Self {
        let ip_address = match nv.network_mode {
            NetworkMode::Dhcp => None,
            NetworkMode::None => None,
            NetworkMode::StaticIp => nv.ip_address.clone(),
        };

        let kernel_params = match &nv.kernel_params {
            Some(kp) => kp.to_owned(),
            None => String::from(DEFAUL_KERNEL_PARAMS),
        };

        Vm {
            id: Uuid::new_v4(),
            name: nv.name.to_owned(),
            status: 0,
            host_id: None,
            vcpu: nv.vcpu,
            memory: nv.memory,
            kernel: nv.kernel,
            ip_address: ip_address,
            mac_address: nv.mac_address.clone(),
            network_mode: nv.network_mode.as_str(),
            kernel_params,
        }
    }
}
