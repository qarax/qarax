use axum::{extract::Path, Extension, Json};
use http::StatusCode;
use ipnetwork::IpNetwork;
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    env::Environment,
    models::networks::{self as network_model, NetworkError, NetworkType, NewNetwork},
};

use super::{ApiResponse, ServerError};

#[tracing::instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<Environment>,
    Json(network_request): Json<NewNetworkRequest>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let new_network: NewNetwork = network_request.try_into()?;
    let network_id = network_model::add(env.db(), &new_network).await?;

    Ok(ApiResponse {
        data: network_id,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn get(
    Extension(env): Extension<Environment>,
    Path(network_id): Path<Uuid>,
) -> Result<ApiResponse<network_model::Network>, ServerError> {
    let network = network_model::by_id(env.db(), &network_id).await?;

    Ok(ApiResponse {
        data: network,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<network_model::Network>>, ServerError> {
    let networks = network_model::list(env.db()).await?;

    Ok(ApiResponse {
        data: networks,
        code: StatusCode::OK,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewNetworkRequest {
    pub mac_address: Option<MacAddress>,
    pub ip_address: Option<IpNetwork>,
    pub network_type: NetworkType,
}

impl TryFrom<NewNetworkRequest> for NewNetwork {
    type Error = NetworkError;

    fn try_from(value: NewNetworkRequest) -> Result<Self, Self::Error> {
        NewNetwork::new(value.mac_address, value.ip_address, value.network_type)
    }
}
