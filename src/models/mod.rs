use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use sqlx::sqlx_macros::Type;
use sqlx::types::Uuid;
use strum_macros::{Display, EnumString};
use thiserror::Error;

pub mod drives;
pub mod hosts;
pub mod kernels;
pub mod storage;
pub mod vms;
pub mod volumes;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Inavlid name: {0}")]
    InvalidName(String),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ValidName(pub String);

impl AsRef<str> for ValidName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl ValidName {
    pub fn new(name: String) -> Result<Self, ValidationError> {
        lazy_static! {
            static ref RE: Regex = Regex::new("^[a-zA-Z0-9-_]+$").unwrap();
        }
        if !RE.is_match(&name) {
            return Err(ValidationError::InvalidName(name));
        }

        Ok(Self(name))
    }
}
