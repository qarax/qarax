use std::collections::BTreeMap;

use crate::handlers::ansible::AnsibleCommand;
use crate::handlers::models::hosts::HostError;

use super::models::hosts::NewHost;
use super::rpc::client::Client;
use super::*;
use axum::extract::{Json, Path};
use models::hosts as host_model;
use models::hosts::Host;
use models::hosts::Status as HostStatus;
use sqlx::PgPool;

#[derive(Debug)]
enum HostHandlerError {
    CannotAddHost(String),
    CannotUpdateHost(Uuid),
    HostNotFound(Uuid),
    NameAlreadyExists(String),
    AddressAlreadyExists(String),
    Other,
}

impl From<HostHandlerError> for ServerError {
    fn from(err: HostHandlerError) -> Self {
        match err {
            HostHandlerError::CannotAddHost(name) => {
                ServerError::Internal(format!("Cannot add host {}", name))
            }
            HostHandlerError::CannotUpdateHost(host_id) => {
                ServerError::Internal(format!("Cannot update host {}", host_id))
            }
            HostHandlerError::AddressAlreadyExists(address) => {
                ServerError::Validation(format!("host address '{}' already exists", address))
            }
            HostHandlerError::HostNotFound(id) => {
                ServerError::EntityNotFound(format!("host id '{}' not found", id))
            }
            HostHandlerError::NameAlreadyExists(name) => {
                ServerError::Validation(format!("address {} already exists", name))
            }
            HostHandlerError::Other => ServerError::Internal(format!("Internal error")),
        }
    }
}

impl From<HostError> for HostHandlerError {
    fn from(err: HostError) -> Self {
        match err {
            HostError::Add(host_id, e) => {
                tracing::error!("cannot add host '{}': {}", host_id, e);
                HostHandlerError::CannotAddHost(host_id)
            }
            HostError::Find(host_id, e @ sqlx::Error::RowNotFound) => {
                tracing::error!("cannot find host '{}': {}", host_id, e);
                HostHandlerError::HostNotFound(host_id)
            }
            HostError::Find(_, e) => {
                tracing::error!("Unexpected error: {}", e);
                HostHandlerError::Other
            }
            HostError::List(e) => {
                tracing::error!("cannot list hosts: {}", e);
                HostHandlerError::Other
            }
            HostError::Update(host_id, e) => {
                tracing::error!("cannot update host '{}': {}", host_id, e);
                HostHandlerError::CannotUpdateHost(host_id)
            }
            HostError::Other(e) => {
                tracing::error!("Unexpected error: {}", e);
                HostHandlerError::Other
            }
        }
    }
}

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Host>>, ServerError> {
    let hosts = host_model::list(env.db())
        .await
        .map_err(HostHandlerError::from)?;

    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(host): Json<NewHost>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    if host_model::by_name(env.db(), &host.name).await.is_ok() {
        Err(HostHandlerError::NameAlreadyExists(host.name.to_string()))?
    }

    if host_model::by_address(env.db(), &host.address)
        .await
        .is_ok()
    {
        Err(HostHandlerError::AddressAlreadyExists(
            host.address.to_string(),
        ))?
    }

    let host_id = host_model::add(env.db(), &host)
        .await
        .map_err(HostHandlerError::from)?;

    Ok(ApiResponse {
        data: host_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<Host>, ServerError> {
    let host = host_model::by_id(env.db(), &host_id)
        .await
        .map_err(HostHandlerError::from)?;

    Ok(ApiResponse {
        data: host,
        code: StatusCode::CREATED,
    })
}

pub async fn install(
    Extension(env): Extension<Environment>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<String>, ServerError> {
    let host = host_model::by_id(env.db(), &host_id)
        .await
        .map_err(HostHandlerError::from)?;

    host_model::update_status(env.db(), host_id, HostStatus::Installing)
        .await
        .map_err(HostHandlerError::from)?;

    let mut extra_params = BTreeMap::new();
    extra_params.insert(String::from("ansible_password"), host.password.to_owned());

    extra_params.insert(
        String::from("fcversion"),
        dotenv::var("FC_VERSION").expect("FC_VERSION is not set!"),
    );

    extra_params.insert(
        String::from("local_node_path"),
        dotenv::var("LOCAL_NODE_PATH").unwrap_or_else(|_| String::from("")),
    );

    tokio::spawn(async move {
        let playbook = AnsibleCommand::new(
            ansible::INSTALL_HOST_PLAYBOOK,
            &host.host_user,
            &host.address,
            extra_params,
        );

        match playbook.run_playbook().await {
            Ok(_) => {
                tracing::info!("Installation successful");
                host_model::update_status(env.db(), host_id, HostStatus::Up)
                    .await
                    .unwrap();
            }

            Err(e) => {
                tracing::error!("Installation failed: {}", e);
                host_model::update_status(env.db(), host_id, HostStatus::InstallationFailed)
                    .await
                    .unwrap();
            }
        }
    });

    Ok(ApiResponse {
        code: StatusCode::ACCEPTED,
        data: String::from("started"),
    })
}

pub async fn find_running_host(pool: &PgPool) -> Result<Host, ServerError> {
    let hosts = host_model::by_status(pool, HostStatus::Up)
        .await
        .map_err(HostHandlerError::from)?;

    if hosts.is_empty() {
        return Err(ServerError::EntityNotFound(String::from("host")));
    }

    Ok(hosts[0].clone())
}

pub async fn initalize_hosts(env: Environment) -> Result<(), ServerError> {
    // TODO: add lookup method that can search multiple statuses
    let running_hosts = host_model::by_status(env.db(), HostStatus::Up)
        .await
        .map_err(HostHandlerError::from)?;

    let unknown_hosts = host_model::by_status(env.db(), HostStatus::Unknown)
        .await
        .map_err(HostHandlerError::from)?;

    if running_hosts.is_empty() && unknown_hosts.is_empty() {
        tracing::info!("No hosts were running or unknown, skipping initialization...");
        return Ok(());
    }

    // TODO: parallelize this
    for host in [running_hosts, unknown_hosts].concat() {
        let env = env.clone();
        tokio::spawn(async move {
            initialize_host(&host, env).await;
        });
    }

    Ok(())
}

pub async fn health_check(
    Extension(env): Extension<Environment>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<String>, ServerError> {
    let host = if let Ok(host) = host_model::by_id(env.db(), &host_id).await {
        host
    } else {
        tracing::error!("Failed to find host: {}", host_id);
        return Err(ServerError::Validation(host_id.to_string()));
    };

    if health_check_internal(&host).await.is_err() {
        tracing::error!("Healthcheck for host: {} failed", host_id);
        return Err(ServerError::Validation(host_id.to_string()));
    }

    Ok(ApiResponse {
        code: StatusCode::OK,
        data: String::from("ok"),
    })
}

async fn health_check_internal(host: &Host) -> Result<(), String> {
    match Client::connect(format!("{}:{}", host.address, host.port).parse().unwrap()).await {
        Ok(client) => {
            if let Err(e) = client.clone().health_check().await {
                tracing::error!("Healthcheck failed: {}", e);
                return Err(e.to_string());
            }

            Ok(())
        }
        Err(e) => {
            tracing::error!("Could not connect to host {}, error: {}", host.id, e);
            return Err(String::from("Could not connect to host"));
        }
    }
}

async fn initialize_host(host: &Host, env: Environment) {
    host_model::update_status(env.db(), host.id, HostStatus::Initializing)
        .await
        .unwrap();
    tracing::info!("Initializing host: {}...", host.id);
    if let Err(e) = health_check_internal(&host).await {
        let _ = host_model::update_status(env.db(), host.id, HostStatus::Unknown).await;
        tracing::error!("Failed to initialize host: {}, error: {}", host.id, e);
        return;
    }

    tracing::info!("Host {} initialized...", host.id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;
    use http::Request;
    use hyper::Body;
    use sqlx::{migrate::MigrateDatabase, postgres, PgPool};
    use tower::ServiceExt;

    use crate::database;

    async fn setup() -> anyhow::Result<PgPool> {
        dotenv().ok();
        let db_url = &dotenv::var("TEST_DATABASE_URL").expect("DATABASE_URL is not set!");
        database::run_migrations(db_url).await.unwrap();
        let pool = database::connect(&db_url).await?;

        Ok(pool)
    }

    async fn teardown(pool: &PgPool) {
        pool.close().await;
        let db_url = &dotenv::var("TEST_DATABASE_URL").expect("DATABASE_URL is not set!");
        postgres::Postgres::drop_database(db_url).await.unwrap();
    }

    #[tokio::test]
    async fn test_add() {
        let pool = setup().await.unwrap();
        let env = Environment::new(pool.clone()).await.unwrap();
        let app = app(env.clone()).await;

        let host = NewHost {
            name: String::from("test_host"),
            address: String::from("127.0.0.1"),
            port: 8080,
            host_user: String::from("root"),
            password: String::from("pass"),
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/hosts")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!(host).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(StatusCode::CREATED, response.status());

        teardown(env.db()).await;
    }
}
