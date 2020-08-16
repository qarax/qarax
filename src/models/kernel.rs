use super::storage::{Storage, StorageConfig};
use super::*;
use crate::schema::kernels;

use uuid::Uuid;

#[derive(Serialize, Queryable, Debug, Insertable, Identifiable, Clone, Associations)]
#[table_name = "kernels"]
#[belongs_to(Storage)]
pub struct Kernel {
    pub id: Uuid,
    pub name: String,
    pub storage_id: Uuid,
}

impl Kernel {
    pub fn all(conn: &PgConnection) -> Result<Vec<Kernel>> {
        use crate::schema::kernels::dsl::*;
        kernels.load::<Kernel>(conn).map_err(|e| anyhow!(e))
    }

    pub fn insert(new_drive: &NewKernel, conn: &PgConnection) -> Result<Uuid> {
        let drive = Kernel::from(new_drive);

        match diesel::insert_into(kernels::table)
            .values(&drive)
            .execute(conn)
        {
            Ok(_) => Ok(drive.id.to_owned()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_storage_config(kernel_id: Uuid, conn: &PgConnection) -> Result<StorageConfig> {
        crate::schema::storage::table
            .inner_join(kernels::table.on(kernels::id.eq(kernel_id)))
            .select(crate::schema::storage::config)
            .get_result::<StorageConfig>(conn)
            .map_err(|e| anyhow!(e))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewKernel {
    pub name: String,
    pub storage_id: Uuid,
}

impl From<&NewKernel> for Kernel {
    fn from(nk: &NewKernel) -> Self {
        Kernel {
            id: Uuid::new_v4(),
            name: nk.name.to_owned(),
            storage_id: nk.storage_id,
        }
    }
}
