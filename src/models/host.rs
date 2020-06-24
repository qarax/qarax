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
    pub status: Status,
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

    pub fn insert(h: &NewHost, conn: &PgConnection) -> Result<uuid::Uuid, String> {
        let h = Host::from(h);

        match diesel::insert_into(hosts::table).values(&h).execute(conn) {
            Ok(_) => Ok(h.id.to_owned()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn by_id(id: Uuid, conn: &PgConnection) -> Result<Host, String> {
        match all_hosts.find(id).first(conn) {
            Ok(h) => Ok(h),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        diesel::delete(all_hosts).execute(conn)
    }
}

impl From<&NewHost> for Host {
    fn from(nh: &NewHost) -> Self {
        Host {
            id: Uuid::new_v4(),
            name: nh.name.to_owned(),
            address: nh.address.to_owned(),
            port: nh.port.to_owned(),
            host_user: nh.user.to_owned(),
            password: nh.password.to_owned(),
            status: Status::Down,
        }
    }
}

use diesel::pg::Pg;
use diesel::sql_types::Varchar;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, ToSql, Output, IsNull};
use std::io::Write;

#[derive(Deserialize, Serialize, Debug, Copy, Clone, AsExpression, FromSqlRow)]
#[sql_type = "Varchar"]
pub enum Status {
    Unknown,
    Down,
    Installing,
    Initializing,
    Up,
}

impl ToSql<Varchar, Pg> for Status {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        match *self {
            Status::Unknown => out.write_all(b"UNKNOWN")?,
            Status::Down => out.write_all(b"DOWN")?,
            Status::Installing => out.write_all(b"INSTALLING")?,
            Status::Initializing => out.write_all(b"INITIALIZING")?,
            Status::Up => out.write_all(b"UP")?,
        }
        Ok(IsNull::No)
    }
}
impl FromSql<Varchar, Pg> for Status {
    fn from_sql(bytes: Option<&[u8]>) -> deserialize::Result<Self> {
        match not_none!(bytes) {
            b"UNKNOWN" => Ok(Status::Unknown),
            b"DOWN" => Ok(Status::Down),
            b"INSTALLING" => Ok(Status::Installing),
            b"INITIALIZING" => Ok(Status::Initializing),
            b"UP" => Ok(Status::Up),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}
