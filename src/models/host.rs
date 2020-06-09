use super::*;
use crate::schema::hosts;
use crate::schema::hosts::dsl::hosts as all_hosts;
use diesel::PgConnection;
use std::convert::From;
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
    pub fn all(conn: &PgConnection) -> Vec<Host> {
        all_hosts
            .order(hosts::id.desc())
            .load::<Host>(conn)
            .unwrap()
    }

    pub fn insert(h: NewHost, conn: &PgConnection) -> Result<uuid::Uuid, String> {
        let h = Host::from(h);

        match diesel::insert_into(hosts::table).values(&h).execute(conn) {
            Ok(_) => Ok(h.id.to_owned()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        diesel::delete(all_hosts).execute(conn)
    }
}

impl From<NewHost> for Host {
    fn from(nh: NewHost) -> Self {
        Host {
            id: Uuid::new_v4(),
            name: nh.name,
            address: nh.address,
            port: nh.port,
            host_user: nh.user,
            password: nh.password,
            status: 0,
        }
    }
}
