mod download;
pub mod handler;

use strum_macros::{Display, EnumString};

pub static STORAGE_PATH: &str = "/home/qarax/storage/";

#[derive(EnumString, Display, PartialEq)]
pub enum StorageType {
    #[strum(serialize = "local")]
    Local,
    #[strum(serialize = "shared")]
    Shared,
}
