use super::storage::Storage;
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

    pub fn insert(new_kernel: &NewKernel, conn: &PgConnection) -> Result<Uuid> {
        let kernel = Kernel::from(new_kernel);

        match diesel::insert_into(kernels::table)
            .values(&kernel)
            .execute(conn)
        {
            Ok(_) => Ok(kernel.id.to_owned()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_storage(kernel_id: Uuid, conn: &PgConnection) -> Result<Storage> {
        crate::schema::storage::table
            .inner_join(kernels::table.on(kernels::id.eq(kernel_id)))
            .select(crate::schema::storage::all_columns)
            .get_result::<Storage>(conn)
            .map_err(|e| anyhow!(e))
    }

    pub fn delete_all(conn: &PgConnection) -> Result<usize, diesel::result::Error> {
        use crate::schema::kernels::dsl::*;

        diesel::delete(kernels).execute(conn)
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
