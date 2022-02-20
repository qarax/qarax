use super::rpc::client::StorageCreateRequest;
use super::*;
use crate::models::storage as storage_model;
use crate::models::storage::{NewStorage, Storage};
use crate::models::storage::{StorageConfig, StorageError, StorageName, StorageType};

use axum::extract::{Json, Path};

#[tracing::instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<Storage>>, ServerError> {
    let storages = storage_model::list(env.db()).await?;

    Ok(ApiResponse {
        data: storages,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env))]
pub async fn add(
    Extension(env): Extension<Environment>,
    Json(storage_request): Json<NewStorageRequest>,
    Extension(request_id): Extension<RequestId>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let storage = storage_request.try_into()?;
    let storage_id = storage_model::add(env.db(), &storage).await?;

    // TODO: perhaps this can be skipped?
    let storage = storage_model::by_id(env.db(), storage_id).await?;

    let clients = &*env.storage_clients().read().await;
    let client = clients.get(&storage.config.host_id.unwrap()).unwrap(); // TODO: handle errors properly

    let request = StorageCreateRequest {
        storage,
        request_id: request_id.into_header_value().to_str().unwrap().to_string(),
    };

    client.create(request).await.unwrap();

    Ok(ApiResponse {
        data: storage_id,
        code: StatusCode::CREATED,
    })
}

#[tracing::instrument(skip(env), fields(storage_id=%storage_id))]
pub async fn get(
    Extension(env): Extension<Environment>,
    Path(storage_id): Path<Uuid>,
) -> Result<ApiResponse<Storage>, ServerError> {
    let storage = storage_model::by_id(env.db(), storage_id).await?;

    Ok(ApiResponse {
        data: storage,
        code: StatusCode::OK,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewStorageRequest {
    name: String,
    storage_type: StorageType,
    config: StorageConfig,
}

impl TryFrom<NewStorageRequest> for NewStorage {
    type Error = StorageError;

    fn try_from(value: NewStorageRequest) -> Result<Self, Self::Error> {
        let name = StorageName::new(value.name)?;
        let storage = NewStorage::new(name, value.storage_type, value.config)?;

        Ok(storage)
    }
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

    use crate::{database, env::Environment, handlers::app};

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
    #[ignore]
    // TODO: this tests cannot pass currently because it tries to
    // access qarax-node. It needs to be mocked or we can just rely on e2e.
    async fn test_add() {
        let pool = setup().await.unwrap();
        let env = Environment::new(pool.clone()).await.unwrap();
        let app = app(env.clone()).await;

        let host = NewStorage {
            name: StorageName::new("test_storage".to_owned()).unwrap(),
            storage_type: StorageType::Local,
            config: StorageConfig {
                host_id: Some(Uuid::new_v4()),
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

        teardown(env.db()).await;
    }
}
