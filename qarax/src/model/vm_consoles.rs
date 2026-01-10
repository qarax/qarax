use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmConsole {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub console_type: String, // "SERIAL" or "CONSOLE"
    pub mode: ConsoleMode,
    pub file_path: Option<String>,
    pub socket_path: Option<String>,
    pub iommu: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "console_mode")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ConsoleMode {
    Off,
    Pty,
    Tty,
    File,
    Socket,
    Null,
}

pub async fn list_by_vm(pool: &PgPool, vm_id: Uuid) -> Result<Vec<VmConsole>, sqlx::Error> {
    let consoles = sqlx::query_as!(
        VmConsole,
        r#"
SELECT id,
        vm_id,
        console_type,
        mode as "mode: _",
        file_path as "file_path?",
        socket_path as "socket_path?",
        iommu as "iommu!"
FROM vm_consoles
WHERE vm_id = $1
ORDER BY console_type
        "#,
        vm_id
    )
    .fetch_all(pool)
    .await?;

    Ok(consoles)
}

pub async fn get(pool: &PgPool, console_id: Uuid) -> Result<VmConsole, sqlx::Error> {
    let console = sqlx::query_as!(
        VmConsole,
        r#"
SELECT id,
        vm_id,
        console_type,
        mode as "mode: _",
        file_path as "file_path?",
        socket_path as "socket_path?",
        iommu as "iommu!"
FROM vm_consoles
WHERE id = $1
        "#,
        console_id
    )
    .fetch_one(pool)
    .await?;

    Ok(console)
}
