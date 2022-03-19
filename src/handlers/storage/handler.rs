use super::local_storage::LocalStorage;
use super::storage::Storage;
use super::*;
use crate::handlers::volumes::NewVolumeRequest;
use crate::models::storage as storage_model;
use crate::models::storage::{NewStorage, Storage as StorageModel};
use crate::models::storage::{StorageConfig, StorageError, StorageName, StorageType};
use crate::models::volumes::NewVolume;

use axum::extract::{Json, Path};

#[tracing::instrument(skip(env))]
pub async fn list(
    Extension(env): Extension<Environment>,
) -> Result<ApiResponse<Vec<StorageModel>>, ServerError> {
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
) -> Result<ApiResponse<Uuid>, ServerError> {
    let new_storage: NewStorage = storage_request.try_into()?;
    let clients = env.storage_clients().read().await;
    let storage_client = clients.get(&new_storage.config.host_id.unwrap()).unwrap();

    let storage = match new_storage.storage_type {
        StorageType::Local => {
            LocalStorage::create(storage_client.clone(), env.db().clone(), new_storage).await?
        }
        StorageType::Shared => todo!(),
    };

    Ok(ApiResponse {
        data: storage.id(),
        code: StatusCode::CREATED,
    })
}

#[tracing::instrument(skip(env), fields(storage_id=%storage_id))]
pub async fn get(
    Extension(env): Extension<Environment>,
    Path(storage_id): Path<Uuid>,
) -> Result<ApiResponse<StorageModel>, ServerError> {
    let storage = storage_model::by_id(env.db(), &storage_id).await?;

    Ok(ApiResponse {
        data: storage,
        code: StatusCode::OK,
    })
}

#[tracing::instrument(skip(env), fields(storage_id=%storage_id))]
pub async fn create_volume(
    Extension(env): Extension<Environment>,
    Path(storage_id): Path<Uuid>,
    Json(volume_request): Json<NewVolumeRequest>,
) -> Result<ApiResponse<Uuid>, ServerError> {
    let new_volume = NewVolume::try_from(volume_request)?;
    let volume_id = create_volume_concrete(&storage_id, new_volume, env).await?;

    Ok(ApiResponse {
        data: volume_id,
        code: StatusCode::OK,
    })
}

pub(crate) async fn create_volume_concrete(
    storage_id: &Uuid,
    new_volume: NewVolume,
    env: Environment,
) -> Result<Uuid, ServerError> {
    let storage: StorageModel = storage_model::by_id(env.db(), &storage_id).await?;
    let concrete_storage = match storage.storage_type {
        StorageType::Local => {
            let clients = env.storage_clients().read().await;
            let storage_client = clients.get(&storage.config.host_id.unwrap()).unwrap();

            LocalStorage::new(storage, storage_client.clone(), env.db().clone())
        }
        StorageType::Shared => todo!(),
    };

    let volume = concrete_storage.create_volume(new_volume).await?;
    Ok(volume.id)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewStorageRequest {
    pub name: String,
    pub storage_type: StorageType,
    pub config: StorageConfig,
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
