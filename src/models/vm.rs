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
    pub kernel: String,
    pub root_file_system: String,
    pub address: Option<String>,
    pub network_mode: Option<String>,
    pub kernel_params: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewVm {
    pub name: String,
    pub vcpu: i32,
    pub memory: i32,
    pub kernel: String,
    pub root_file_system: String,
    pub network_mode: Option<NetworkMode>, // TODO: remove option and use (DHCP, STATIC_IP, NONE)
    pub address: Option<String>,
    pub kernel_params: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum NetworkMode {
    #[serde(rename = "dhcp")]
    Dhcp,
    #[serde(rename = "static_ip")]
    StaticIp,
    // TODO: add default None
}

impl NetworkMode {
    pub fn as_str(&self) -> String {
        match self {
            NetworkMode::Dhcp => String::from("dhcp"),
            NetworkMode::StaticIp => String::from("static_ip"),
        }
    }
}

impl FromStr for NetworkMode {
    type Err = ();
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "dhcp" => Ok(NetworkMode::Dhcp),
            "static_ip" => Ok(NetworkMode::StaticIp),
            _ => Err(()),
        }
    }
}

impl Vm {
    pub fn all(conn: &PgConnection) -> Vec<Vm> {
        use crate::schema::vms::dsl::*;
        vms.load::<Vm>(conn).unwrap()
    }

    pub fn by_id(vm_id: Uuid, conn: &PgConnection) -> Result<Vm, String> {
        use crate::schema::vms::dsl::*;

        match vms.find(vm_id).first(conn) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn insert(v: &NewVm, conn: &PgConnection) -> Result<uuid::Uuid, String> {
        let v = Vm::from(v);

        match diesel::insert_into(vms::table).values(&v).execute(conn) {
            Ok(_) => Ok(v.id.to_owned()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn update(vm: &Vm, conn: &PgConnection) -> Result<Vm, String> {
        match diesel::update(vm).set(vm).get_result(conn) {
            Ok(host) => Ok(host),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        use crate::schema::vms::dsl::*;

        diesel::delete(vms).execute(conn)
    }
}

impl From<&NewVm> for Vm {
    fn from(nv: &NewVm) -> Self {
        let network_mode: Option<String>;
        let address = if let Some(n) = &nv.network_mode {
            network_mode = Some(n.as_str());
            match n {
                NetworkMode::Dhcp => String::from(""),
                NetworkMode::StaticIp => nv.address.as_ref().unwrap().to_owned(),
            }
        } else {
            network_mode = None;
            String::from("")
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
            kernel: nv.kernel.to_owned(),
            root_file_system: nv.root_file_system.to_owned(),
            address: Some(address),
            network_mode,
            kernel_params,
        }
    }
}
