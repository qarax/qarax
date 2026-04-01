/// Shared OCI image configuration parsing structures.
///
/// Used by both `image_store/manager.rs` and `overlaybd/manager.rs` to deserialize
/// the OCI image config blob fetched from a registry.
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct OciImageConfig {
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub config: OciImageConfigDetails,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct OciImageConfigDetails {
    pub env: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
}
