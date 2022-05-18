use crate::handlers::ServerError;

use super::*;

use ipnetwork::IpNetwork;
use mac_address::MacAddress;
// VM -> network mapping will contain is_root to select which network is used in boot parameters
// Need to validate there is only one root network

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Invalid config {0}")]
    InvalidConfig(String),
    #[error("Unexpected failure: {0}")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<NetworkError> for ServerError {
    fn from(e: NetworkError) -> Self {
        match e {
            NetworkError::InvalidConfig(e) => {
                ServerError::Validation(format!("Invalid config {e}"))
            }
            NetworkError::Other(e) => ServerError::Internal(e.to_string()),
        }
    }
}

impl From<sqlx::Error> for NetworkError {
    fn from(e: sqlx::Error) -> Self {
        NetworkError::Other(Box::new(e))
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Network {
    pub id: Uuid,
    pub mac_address: Option<MacAddress>,
    pub ip_address: Option<IpNetwork>,
    pub network_type: NetworkType,
}

#[derive(Serialize, Debug, Clone)]
pub struct NewNetwork {
    pub mac_address: Option<MacAddress>,
    pub ip_address: Option<IpNetwork>,
    pub network_type: NetworkType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Type, EnumString, Display)]
#[sqlx(rename_all = "lowercase")]
#[sqlx(type_name = "varchar")]
#[strum(serialize_all = "snake_case")]
pub enum NetworkType {
    Static,
    Dhcp,
}

impl NewNetwork {
    pub fn new(
        mac_address: Option<MacAddress>,
        ip_address: Option<IpNetwork>,
        network_type: NetworkType,
    ) -> Result<Self, NetworkError> {
        if network_type == NetworkType::Static && (mac_address.is_none() || ip_address.is_none()) {
            return Err(NetworkError::InvalidConfig(String::from(
                "mac_address and ip_address must be set for static network",
            )));
        }

        Ok(NewNetwork {
            mac_address,
            ip_address,
            network_type,
        })
    }
}

pub async fn add(pool: &PgPool, network: &NewNetwork) -> Result<Uuid, NetworkError> {
    let rec = sqlx::query!(
        r#"
INSERT INTO networks (mac_address, ip_address, type)
VALUES ( $1, $2, $3)
RETURNING id
        "#,
        network.mac_address.unwrap(),
        network.ip_address.unwrap(),
        network.network_type.to_string(),
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn by_id(pool: &PgPool, network_id: &Uuid) -> Result<Network, NetworkError> {
    let network = sqlx::query_as!(
        Network,
        r#"
SELECT id, mac_address, ip_address, type as "network_type: _"
FROM networks
WHERE id = $1
        "#,
        network_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(network)
}

pub async fn list(pool: &PgPool) -> Result<Vec<Network>, NetworkError> {
    let networks = sqlx::query_as!(
        Network,
        r#"
        SELECT id, mac_address, ip_address, type as "network_type: _"
        FROM networks
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(networks)
}
