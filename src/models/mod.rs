use anyhow::{anyhow, Error};
use diesel::dsl::*;
use diesel::*;
use serde::{Deserialize, Serialize};
use std::convert::From;
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

pub mod host;
pub mod vm;

#[derive(Debug)]
struct EntityId(Uuid);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

impl From<Uuid> for EntityId {
    fn from(uuid: Uuid) -> Self {
        EntityId { 0: uuid }
    }
}

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
    NotFound(EntityType, EntityId, Error),
    #[error("Failed to add '{0}' '{1}' to the database, error {2}")]
    FailedToAdd(EntityType, EntityId, Error),
}
