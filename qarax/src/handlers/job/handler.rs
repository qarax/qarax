use axum::{Extension, extract::Path};
use http::StatusCode;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    App,
    model::jobs::{self, Job},
};

use super::{ApiResponse, Result};

#[utoipa::path(
    get,
    path = "/v1/jobs/{job_id}",
    params(
        ("job_id" = uuid::Uuid, Path, description = "Job unique identifier")
    ),
    responses(
        (status = 200, description = "Job details", body = Job),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "jobs"
)]
#[instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<App>,
    Path(job_id): Path<Uuid>,
) -> Result<ApiResponse<Job>> {
    let job = jobs::get(env.pool(), job_id).await?;
    Ok(ApiResponse {
        data: job,
        code: StatusCode::OK,
    })
}
