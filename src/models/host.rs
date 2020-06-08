use super::*;
use crate::database::Connection;
use crate::schema::hosts;
use crate::schema::hosts::dsl::{hosts as all_hosts};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Queryable, Debug, Insertable)]
#[table_name = "hosts"]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: i32, // TODO: make enum
    pub host_user: String,

    #[serde(skip_serializing)]
    pub password: String,
}


#[derive(Serialize, Deserialize, Debug)]
pub struct NewHost {
    pub name: String,
    pub address: String,
    pub port: i32,
    pub user: String,
    pub password: String,
    pub local_node_path: String,
}

impl Host {
    pub fn all(conn: Connection) -> Vec<Host> {
        all_hosts.order(hosts::id.desc()).load::<Host>(&*conn).unwrap()
    }

    pub fn insert(h: &NewHost, conn: Connection) -> Result<String, String> {
        let host_id = Uuid::new_v4();

        let h = Host {
            id: host_id,
            name: h.name.clone(), // There's probably a better way
            address: h.address.clone(),
            port: h.port,
            host_user: h.user.clone(),
            password: h.password.clone(),
            status: 0,
        };

        match diesel::insert_into(hosts::table).values(h).execute(&*conn) {
            Ok(_) => Ok(host_id.to_string()),
            Err(e) => Err(e.to_string())
        }
    }
}
