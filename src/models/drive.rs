use super::storage::Storage;
use super::vm::Vm;
use super::*;
use crate::schema::drives;
use crate::schema::vm_drives_map;
use uuid::Uuid;

#[derive(Serialize, Queryable, Debug, Insertable, Identifiable, Clone)]
#[table_name = "drives"]
pub struct Drive {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub readonly: bool,
    pub rootfs: bool,
    pub storage_id: Uuid,
}

#[derive(Serialize, Queryable, Debug, Insertable, Identifiable, Clone, Associations)]
#[table_name = "vm_drives_map"]
#[primary_key(vm_id, drive_id)]
#[belongs_to(Drive)]
#[belongs_to(Vm)]
pub struct AttachedDrive {
    pub vm_id: Uuid,
    pub drive_id: Uuid,
}

impl Drive {
    pub fn all(conn: &PgConnection) -> Result<Vec<Drive>> {
        use crate::schema::drives::dsl::*;
        drives.load::<Drive>(conn).map_err(|e| anyhow!(e))
    }

    pub fn get_drives_for_vm(vm: &vm::Vm, conn: &PgConnection) -> Result<Vec<Drive>> {
        AttachedDrive::belonging_to(vm)
            .inner_join(drives::table)
            .select(drives::all_columns)
            .load::<Drive>(conn)
            .map_err(|e| anyhow!("Could not fetch drives for VM: {}", e))
    }

    pub fn insert(new_drive: &NewDrive, conn: &PgConnection) -> Result<Uuid> {
        let drive = Drive::from(new_drive);

        match diesel::insert_into(drives::table)
            .values(&drive)
            .execute(conn)
        {
            Ok(_) => Ok(drive.id.to_owned()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_storage(drive_id: Uuid, conn: &PgConnection) -> Result<Storage> {
        crate::schema::storage::table
            .inner_join(drives::table.on(drives::id.eq(drive_id)))
            .select(crate::schema::storage::all_columns)
            .get_result::<Storage>(conn)
            .map_err(|e| anyhow!(e))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewDrive {
    pub name: String,
    pub readonly: bool,
    pub rootfs: bool,
    pub storage_id: Uuid,
}

impl From<&NewDrive> for Drive {
    fn from(nd: &NewDrive) -> Self {
        Drive {
            id: Uuid::new_v4(),
            name: nd.name.to_owned(),
            status: String::from("Down"),
            readonly: nd.readonly,
            rootfs: nd.rootfs,
            storage_id: nd.storage_id,
        }
    }
}
