use super::*;
use crate::schema::hosts;
use crate::schema::hosts::dsl::*;
use anyhow::anyhow;
use anyhow::Result;
use diesel::PgConnection;
use std::convert::From;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Queryable, Debug, Insertable, Identifiable, Clone)]
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
    pub local_node_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstallHost {
    pub local_node_path: String,

    #[serde(rename = "fcversion")]
    pub fc_version: String,
}

impl Host {
    pub fn all(conn: &PgConnection) -> Result<Vec<Host>> {
        use crate::schema::hosts::dsl::*;

        hosts
            .order(crate::schema::hosts::id.desc())
            .load::<Host>(conn)
            .map_err(|e| anyhow!(e))
    }

    pub fn by_id(host_id: Uuid, conn: &PgConnection) -> Result<Host> {
        match hosts.find(host_id).first(conn) {
            Ok(h) => Ok(h),
            Err(e) => {
                Err(
                    ModelError::NotFound(EntityType::Host, host_id.to_string(), e.to_string())
                        .into(),
                )
            }
        }
    }

    pub fn by_status(host_status: Status, conn: &PgConnection) -> Result<Vec<Host>, String> {
        match hosts.filter(host::status.eq(host_status)).get_results(conn) {
            Ok(h) => Ok(h),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn insert(h: &NewHost, conn: &PgConnection) -> Result<uuid::Uuid> {
        let h = Host::from(h);

        match diesel::insert_into(hosts::table).values(&h).execute(conn) {
            Ok(_) => Ok(h.id.to_owned()),
            Err(e) => {
                Err(
                    ModelError::FailedToAdd(EntityType::Host, h.id.to_string(), e.to_string())
                        .into(),
                )
            }
        }
    }

    pub fn update_status(
        h: &Host,
        new_status: Status,
        conn: &PgConnection,
    ) -> Result<Host, String> {
        match diesel::update(h)
            .set(status.eq(new_status))
            .get_result(conn)
        {
            Ok(host) => Ok(host),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        use crate::schema::hosts::dsl::*;

        diesel::delete(hosts).execute(conn)
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

use diesel::deserialize::{self, FromSql};
use diesel::pg::Pg;
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Varchar;
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
