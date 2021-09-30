use super::*;

use axum::extract::{Json, Path};

use models::storage as storage_model;
use models::storage::{NewStorage, Storage};

pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Storage>>, ServerError> {
    let storages = storage_model::list(env.db()).await.map_err(|e| {
        tracing::error!("Failed to list storages, error: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: storages,
        code: StatusCode::OK,
    })
}

pub async fn add(
    Extension(env): Extension<Environment>,
    Json(storage): Json<NewStorage>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let storage_id = storage_model::add(env.db(), &storage).await.map_err(|e| {
        tracing::error!("Can't add storage: {}", e);
        ServerError::Internal
    })?;

    Ok(ApiResponse {
        data: storage_id,
        code: StatusCode::OK,
    })
}

pub async fn get(
    Extension(env): Extension<Environment>,
    Path(storage_id): Path<Uuid>,
) -> Result<ApiResponse<Storage>, ServerError> {
    let storage = storage_model::by_id(env.db(), storage_id)
        .await
        .map_err(|e| {
            tracing::error!("Can't find storage, error: {}", e);
            ServerError::Internal
        })?;

    Ok(ApiResponse {
        data: storage,
        code: StatusCode::CREATED,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;
    use http::{Request, StatusCode};
    use hyper::Body;
    use serde_json::json;
    use sqlx::{migrate::MigrateDatabase, postgres, PgPool};
    use tower::ServiceExt;

    use crate::{
        database,
        env::Environment,
        handlers::{
            app,
            models::storage::{StorageConfig, StorageType},
        },
    };

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

        let host = NewStorage {
            name: "test_storage".to_owned(),
            storage_type: StorageType::Local,
            config: StorageConfig {
                host_id: None,
                path: Some("/tmp/test_storage".to_owned()),
                pool_name: None,
            },
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/storage")
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
