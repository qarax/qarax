pub mod storage_handler;

pub static STORAGE_PATH: &str = "/home/qarax/storage/";

enum StorageType {
    Local,
    Shared,
}
