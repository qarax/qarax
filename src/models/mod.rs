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
