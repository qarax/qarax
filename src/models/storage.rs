use super::*;
use crate::schema::storage;
use diesel::deserialize::FromSql;
use diesel::pg::Pg;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::{Jsonb, Varchar};
use std::convert::From;
use std::io::Write;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Queryable, Debug, Insertable, Identifiable, Clone)]
#[table_name = "storage"]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewStorage {
    pub name: String,
    pub storage_type: StorageType,
    pub config: StorageConfig,
}

impl Storage {
    pub fn all(conn: &PgConnection) -> Result<Vec<Storage>> {
        use crate::schema::storage::dsl::*;
        storage.load::<Storage>(conn).map_err(|e| anyhow!(e))
    }

    pub fn insert(s: &NewStorage, conn: &PgConnection) -> Result<uuid::Uuid> {
        let s = Storage::from(s);

        match diesel::insert_into(storage::table).values(&s).execute(conn) {
            Ok(_) => Ok(s.id.to_owned()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        use crate::schema::storage::dsl::*;

        diesel::delete(storage).execute(conn)
    }
}

impl From<&NewStorage> for Storage {
    fn from(ns: &NewStorage) -> Self {
        Storage {
            id: Uuid::new_v4(),
            name: ns.name.to_owned(),
            status: String::from("UP"),
            storage_type: ns.storage_type,
            config: ns.config.clone(),
        }
    }
}

#[derive(FromSqlRow, Serialize, Deserialize, Debug, AsExpression, Clone, QueryableByName)]
#[sql_type = "Jsonb"]
pub struct StorageConfig {
    #[sql_type = "Uuid"]
    pub host_id: Option<Uuid>,
    #[sql_type = "Uuid"]
    pub path: Option<String>,
    #[sql_type = "Varchar"]
    pub pool_name: Option<String>,
}

impl FromSql<Jsonb, Pg> for StorageConfig {
    fn from_sql(bytes: Option<&[u8]>) -> diesel::deserialize::Result<Self> {
        let value = <serde_json::Value as FromSql<Jsonb, Pg>>::from_sql(bytes)?;
        Ok(serde_json::from_value(value)?)
    }
}

impl ToSql<Jsonb, Pg> for StorageConfig {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> diesel::serialize::Result {
        let value = serde_json::to_value(self)?;
        <serde_json::Value as ToSql<Jsonb, Pg>>::to_sql(&value, out)
    }
}

#[derive(Serialize, Deserialize, Debug, AsExpression, FromSqlRow, Clone, Copy, PartialEq)]
#[sql_type = "Varchar"]
pub enum StorageType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "shared")]
    Shared,
}

impl ToSql<Varchar, Pg> for StorageType {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Pg>) -> serialize::Result {
        match *self {
            StorageType::Local => out.write_all(b"LOCAL")?,
            StorageType::Shared => out.write_all(b"SHARED")?,
        }
        Ok(IsNull::No)
    }
}
impl FromSql<Varchar, Pg> for StorageType {
    fn from_sql(bytes: Option<&[u8]>) -> deserialize::Result<Self> {
        match not_none!(bytes) {
            b"LOCAL" => Ok(StorageType::Local),
            b"SHARED" => Ok(StorageType::Shared),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}
