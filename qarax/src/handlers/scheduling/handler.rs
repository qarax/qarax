use axum::Extension;
use http::StatusCode;
use tracing::instrument;

use crate::{App, configuration::SchedulingSettings};

use super::{ApiResponse, Result};

#[utoipa::path(
    get,
    path = "/scheduling/config",
    responses(
        (status = 200, description = "Current scheduling configuration", body = SchedulingSettings)
    ),
    tag = "scheduling"
)]
#[instrument(skip(env))]
pub async fn config(Extension(env): Extension<App>) -> Result<ApiResponse<SchedulingSettings>> {
    Ok(ApiResponse {
        data: env.scheduling().clone(),
        code: StatusCode::OK,
    })
}
