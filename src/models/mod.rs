use anyhow::{anyhow, Error, Result};
use diesel::dsl::*;
use diesel::*;
use serde::{Deserialize, Serialize};
use std::convert::From;
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

pub mod drive;
pub mod host;
pub mod kernel;
pub mod storage;
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
            EntityType::Host => write!(f, "Host"),
            EntityType::Vm => write!(f, "VM"),
        }
    }
}

#[derive(Error, Debug)]
enum ModelError {
    #[error("'{0}' '{1}' not found, error {2}")]
    NotFound(EntityType, EntityId, Error),
    #[error("Failed to add '{0}' '{1}' to the database, error {2}")]
    FailedToAdd(EntityType, EntityId, Error),
    #[error("Failed to update '{0}' '{1}' to the database, error {2}")]
    FailedToUpdate(EntityType, EntityId, Error),
    #[error("Could not get '{0}' results, error {1}")]
    NoResults(EntityType, Error),
}
