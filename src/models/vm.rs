use super::*;
use crate::schema::vms;
use diesel::PgConnection;
use std::convert::From;
use uuid::Uuid;

#[derive(Insertable, Identifiable, Serialize, Deserialize, Queryable, Debug)]
#[table_name = "vms"]
pub struct Vm {
    pub id: Uuid,
    pub name: String,
    pub status: i32,
    pub host_id: Option<Uuid>,
    pub vcpu: i32,
    pub memory: i32,
    pub kernel: String,
    pub root_file_system: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewVm {
    pub name: String,
    pub vcpu: i32,
    pub memory: i32,
    pub kernel: String,
    pub root_file_system: String,
}

impl Vm {
    pub fn all(conn: &PgConnection) -> Vec<Vm> {
        use crate::schema::vms::dsl::*;
        vms.load::<Vm>(conn).unwrap()
    }

    pub fn insert(v: &NewVm, conn: &PgConnection) -> Result<uuid::Uuid, String> {
        let v = Vm::from(v);

        match diesel::insert_into(vms::table).values(&v).execute(conn) {
            Ok(_) => Ok(v.id.to_owned()),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl From<&NewVm> for Vm {
    fn from(nv: &NewVm) -> Self {
        Vm {
            id: Uuid::new_v4(),
            name: nv.name.to_owned(),
            status: 0,
            host_id: None,
            vcpu: nv.vcpu,
            memory: nv.memory,
            kernel: nv.kernel.to_owned(),
            root_file_system: nv.root_file_system.to_owned(),
        }
    }
}
