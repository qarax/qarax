use uuid::Uuid;
use super::*;
use crate::schema::hosts::dsl::*;
use crate::database::Connection;
use crate::schema::hosts;


#[derive(Serialize, Deserialize, Queryable, Debug, Insertable)]
#[table_name = "hosts"]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: i32,
    pub status: i32,
    pub host_user: String,
    pub password: String,
}

impl Host {
    pub fn all(conn: Connection) -> Vec<Host> {
        hosts.order(hosts::id.desc()).load::<Host>(&*conn).unwrap()
    }
}
