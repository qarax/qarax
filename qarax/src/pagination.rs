//! Cursor pagination for the lists that grow with use. Each list has a fixed
//! order (by name, or newest first), and a page continues after the last item
//! of the previous one, so items created or deleted meanwhile don't shift the
//! others between pages.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::errors::Error;

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 1000;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Maximum number of items to return, 1 to 1000 (default 100)
    pub limit: Option<u32>,
    /// The `next_cursor` of the previous page; omit it for the first page
    pub cursor: Option<String>,
}

/// A page being built: the rows of one page and the cursor after them. List
/// handlers return it as the list's named page type (see [`page_types`]).
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// The rows a list query returns: those after the previous page's last item
/// in the list's order (bind [`Self::after_key`] and [`Self::after_id`]), at
/// most [`Self::fetch_limit`] of them.
#[derive(Debug)]
pub struct PageRequest {
    after: Option<(String, Uuid)>,
    /// None for [`Self::all`].
    limit: Option<u32>,
}

impl PageRequest {
    /// Every row, for callers inside the control plane.
    pub fn all() -> Self {
        Self {
            after: None,
            limit: None,
        }
    }

    /// The sort key of the previous page's last item: its name, or its
    /// creation time ([`time_key`]) in a list ordered newest first.
    pub fn after_key(&self) -> Option<&str> {
        self.after.as_ref().map(|(key, _)| key.as_str())
    }

    pub fn after_id(&self) -> Option<Uuid> {
        self.after.as_ref().map(|(_, id)| *id)
    }

    /// The query's `LIMIT`: one row more than the page holds, which tells
    /// whether another page follows. NULL (no limit) for [`Self::all`].
    pub fn fetch_limit(&self) -> Option<i64> {
        self.limit.map(|limit| i64::from(limit) + 1)
    }
}

impl TryFrom<PageQuery> for PageRequest {
    type Error = Error;

    fn try_from(query: PageQuery) -> Result<Self, Error> {
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(Error::UnprocessableEntity(format!(
                "limit must be between 1 and {MAX_LIMIT}"
            )));
        }
        let after = query.cursor.as_deref().map(decode_cursor).transpose()?;
        Ok(Self {
            after,
            limit: Some(limit),
        })
    }
}

impl<T> Page<T> {
    /// The page among `rows`, as fetched for `page`, and the cursor after
    /// its last item when more follow. `key` gives a row's sort key and id.
    pub fn new(mut rows: Vec<T>, page: &PageRequest, key: impl Fn(&T) -> (String, Uuid)) -> Self {
        let next_cursor = match page.limit {
            Some(limit) if rows.len() > limit as usize => {
                rows.truncate(limit as usize);
                rows.last().map(|row| {
                    let (key, id) = key(row);
                    hex::encode(format!("{id}:{key}"))
                })
            }
            _ => None,
        };
        Self {
            items: rows,
            next_cursor,
        }
    }
}

/// The sort key of an item in a list ordered newest first. Microseconds,
/// Postgres' precision, so the cursor compares equal to the row it came from.
pub fn time_key(created_at: DateTime<Utc>) -> String {
    created_at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn decode_cursor(cursor: &str) -> Result<(String, Uuid), Error> {
    let invalid = || Error::UnprocessableEntity("invalid cursor".into());
    let text = hex::decode(cursor)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(invalid)?;
    let (id, key) = text.split_once(':').ok_or_else(invalid)?;
    Ok((key.to_string(), id.parse().map_err(|_| invalid())?))
}

/// Declares each list's page type, `NamePage(Item)`: what its handler returns
/// and the API docs describe. A generic page in the docs would copy the item
/// schema into each page instead of referring to it, and the SDK would get a
/// second class per item type.
macro_rules! page_types {
    ($($name:ident($item:ty),)*) => {$(
        /// One page of a list. Pass `next_cursor` as `cursor` to get the next
        /// page; it is null on the last one.
        #[derive(Debug, Serialize, ToSchema)]
        pub struct $name {
            pub items: Vec<$item>,
            pub next_cursor: Option<String>,
        }

        impl $name {
            /// See [`Page::new`].
            pub fn new(
                rows: Vec<$item>,
                page: &PageRequest,
                key: impl Fn(&$item) -> (String, Uuid),
            ) -> Self {
                let Page { items, next_cursor } = Page::new(rows, page, key);
                Self { items, next_cursor }
            }
        }
    )*};
}

page_types! {
    AuditLogPage(crate::model::audit_log::AuditLog),
    BackupPage(crate::model::backups::Backup),
    BootSourcePage(crate::model::boot_sources::BootSource),
    HostPage(crate::model::hosts::Host),
    InstanceTypePage(crate::model::instance_types::InstanceType),
    LifecycleHookPage(crate::model::lifecycle_hooks::LifecycleHook),
    HookExecutionPage(crate::model::lifecycle_hooks::HookExecution),
    NetworkPage(crate::model::networks::Network),
    SandboxPage(crate::model::sandboxes::Sandbox),
    SandboxPoolPage(crate::model::sandbox_pools::SandboxPool),
    SecurityGroupPage(crate::model::security_groups::SecurityGroup),
    StorageObjectPage(crate::model::storage_objects::StorageObject),
    StoragePoolPage(crate::model::storage_pools::StoragePool),
    TransferPage(crate::model::transfers::Transfer),
    VmTemplatePage(crate::model::vm_templates::VmTemplate),
    VmPage(crate::model::vms::Vm),
}
