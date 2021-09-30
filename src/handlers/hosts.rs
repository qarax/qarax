use std::collections::BTreeMap;

use crate::handlers::ansible::AnsibleCommand;
use crate::handlers::models::hosts::HostError;

use super::models::hosts::{NewHost, Status};
use super::rpc::client::Client;
use super::*;
use axum::extract::{Json, Path};
use models::hosts as host_model;
use models::hosts::Host;
use sqlx::PgPool;

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Host>>, ServerError> {
    tracing::info!("list works");
    let hosts = host_model::list(env.db()).await.map_err(|e| {
        tracing::error!("Failed to list hosts, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: hosts,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(host): Json<NewHost>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let host_id = host_model::add(env.db(), &host).await.map_err(|e| {
        tracing::error!("Failed to add host {}", e);
        ServerError::Validation(e.to_string())
    })?;

    Ok(ApiResponse {
        data: host_id,
        code: StatusCode::CREATED,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<Host>, ServerError> {
    let host = host_model::by_id(env.db(), &host_id).await.map_err(|e| {
        tracing::error!("Failed to find host: {}, error:{}", host_id, e);

        match e {
            HostError::Find(id, sqlx::Error::RowNotFound) => {
                ServerError::EntityNotFound(id.to_string())
            }
            _ => ServerError::Internal,
        }
    })?;

    Ok(ApiResponse {
        data: host,
        code: StatusCode::CREATED,
    })
}

pub async fn install(
    Extension(env): Extension<Environment>,
    Path(host_id): Path<Uuid>,
) -> Result<ApiResponse<String>, ServerError> {
    let host = host_model::by_id(env.db(), &host_id).await.map_err(|e| {
        tracing::error!("Failed to find host: {}, error:{}", host_id, e);

        match e {
            HostError::Find(id, sqlx::Error::RowNotFound) => {
                ServerError::EntityNotFound(id.to_string())
            }
            _ => ServerError::Internal,
        }
    })?;

    host_model::update_status(env.db(), host_id, Status::Installing)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update host: {}, error:{}", host_id, e);
            ServerError::Internal
        })?;

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
                host_model::update_status(env.db(), host_id, Status::Up)
                    .await
                    .unwrap();
            }

            Err(e) => tracing::error!("Installation failed: {}", e),
        }
    });

    Ok(ApiResponse {
        code: StatusCode::ACCEPTED,
        data: String::from("started"),
    })
}

pub async fn find_running_host(pool: &PgPool) -> Result<Host, ServerError> {
    let hosts = host_model::by_status(pool, host_model::Status::Up)
        .await
        .map_err(|e| {
            tracing::error!("Failed to a suitable host, error:{}", e);
            ServerError::Internal
        })?;

    if hosts.is_empty() {
        return Err(ServerError::EntityNotFound(String::from("host")));
    }

    Ok(hosts[0].clone())
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

    match Client::connect(format!("{}:{}", host.address, host.port).parse().unwrap()).await {
        Ok(client) => {
            health_check_internal(&client).await.map_err(|e| {
                tracing::error!("Failed to health check host: {}, error:{}", host_id, e);
                ServerError::Internal
            })?;

            env.clients().write().await.insert(host_id, client);

            Ok(ApiResponse {
                code: StatusCode::OK,
                data: String::from("ok"),
            })
        }
        Err(e) => {
            tracing::error!("Failed to health check host: {}, error:{}", host_id, e);
            Err(ServerError::Internal)
        }
    }
}

async fn health_check_internal(client: &Client) -> Result<String, String> {
    let response = client.clone().health_check().await;
    match response {
        Ok(_) => Ok(String::from("OK")),
        Err(e) => {
            tracing::error!("failed {}", e);
            Err(String::from("Failed"))
        }
    }
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
        let app = app(env.clone());

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

        teardown(&env.db()).await;
    }
}
