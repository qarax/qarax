use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{AuthSettings, DatabaseSettings, default_control_plane_architecture},
    startup::run,
};
use reqwest::StatusCode;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    pub db_name: String,
    pub address: String,
}

static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    let mut connection = PgConnection::connect(&config.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.name).as_str())
        .await
        .expect("Failed to create database.");
    let connection_pool = PgPool::connect(&config.connection_string())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("../migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");
    connection_pool
}

async fn spawn_app(auth: AuthSettings) -> TestApp {
    Lazy::force(&TRACING);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{}", port);
    let mut configuration =
        qarax::configuration::get_configuration().expect("Failed to read configuration.");
    configuration.database.name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
        configuration.database.clone(),
        configuration.vm_defaults.clone(),
        configuration.scheduling.clone(),
        configuration.sandbox.clone(),
        auth,
        configuration.ha.clone(),
        default_control_plane_architecture(),
    )
    .await
    .unwrap();
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        let _ = rt.block_on(async move { server.await });
    });
    TestApp {
        db_name: configuration.database.name,
        address,
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let db_name = self.db_name.clone();
        std::thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let config = qarax::configuration::get_configuration()
                    .expect("Failed to read configuration");
                let mut conn = PgConnection::connect_with(&config.database.without_db())
                    .await
                    .expect("Failed to connect to Postgres");
                conn.execute(&*format!("DROP DATABASE \"{}\" WITH (FORCE)", db_name))
                    .await
                    .expect("Failed to drop database.");
                let _ = tx.send(());
            })
        });
        let _ = rx.recv();
    }
}

#[tokio::test]
async fn test_auth_disabled_allows_unauthenticated_requests() {
    let app = spawn_app(AuthSettings {
        enabled: false,
        tokens: vec![],
    })
    .await;

    let res = reqwest::get(format!("{}/hosts", app.address))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_enabled_rejects_missing_token() {
    let app = spawn_app(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let res = reqwest::get(format!("{}/hosts", app.address))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_enabled_rejects_wrong_token() {
    let app = spawn_app(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/hosts", app.address))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_enabled_accepts_valid_token() {
    let app = spawn_app(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/hosts", app.address))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_enabled_allows_public_paths_without_token() {
    let app = spawn_app(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let res = reqwest::get(&app.address).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = reqwest::get(format!("{}/swagger-ui", app.address))
        .await
        .unwrap();
    assert!(res.status().is_success() || res.status().is_redirection());

    let res = reqwest::get(format!("{}/api-docs/openapi.json", app.address))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
