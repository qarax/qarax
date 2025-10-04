use axum::Extension;
use http::StatusCode;
use tracing::instrument;

use crate::{
    App,
    model::vms::{self, Vm},
};

use super::{ApiResponse, Result};

#[instrument(skip(env))]
pub async fn list(Extension(env): Extension<App>) -> Result<ApiResponse<Vec<Vm>>> {
    let hosts = vms::list(env.pool()).await?;
    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

#[instrument(skip(env))]
pub async fn get(Extension(env): Extension<App>, vm_id: uuid::Uuid) -> Result<ApiResponse<Vm>> {
    let vm = vms::get(env.pool(), vm_id).await?;
    Ok(ApiResponse {
        data: vm,
        code: StatusCode::OK,
    })
}
