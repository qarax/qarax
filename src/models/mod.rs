use diesel::dsl::*;
use diesel::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

pub mod host;
pub mod vm;

#[derive(Debug)]
enum EntityType {
    Host,
    Vm,
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Host => write!(f, "Host"),
            Vm => write!(f, "VM"),
        }
    }
}

#[derive(Error, Debug)]
enum ModelError {
    #[error("'{0}' '{1}' not found, error {2}")]
    NotFound(EntityType, String, String),
    #[error("Failed to add '{0}' '{1}' to the database, error {2}")]
    FailedToAdd(EntityType, String, String),
}
