use diesel::dsl::*;
use diesel::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod host;
pub mod vm;

#[derive(Error, Debug)]
enum ModelError {
    #[error("'{0}' '{1}' not found, error {2}")]
    NotFound(String, String, String),
    #[error("Failed to add '{0} '{1}' to the database, error {2}")]
    FailedToAdd(String, String, String),
}
