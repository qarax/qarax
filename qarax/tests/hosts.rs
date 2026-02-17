use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, get_configuration},
    model::hosts::NewHost,
    startup::run,
};
use reqwest::StatusCode;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    pub db_name: String,
    pub address: String,
    pub _pool: PgPool,
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

async fn spawn_app() -> TestApp {
    Lazy::force(&TRACING);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{}", port);
    println!("Address: {}", address);
    let mut configuration =
        qarax::configuration::get_configuration().expect("Failed to read configuration.");
    configuration.database.name = Uuid::new_v4().to_string();
    tracing::info!("Using database {}", configuration.database.name);
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
        configuration.qarax_node.address(),
        configuration.vm_defaults.clone(),
    )
    .await;
    tokio::spawn(async move {
        let _ = server.unwrap().await;
    });
    TestApp {
        db_name: configuration.database.name,
        address,
        _pool: connection_pool,
    }
}

#[tokio::test]
async fn test_list_hosts_empty() {
    let app = spawn_app().await;
    let res: Result<reqwest::Response, reqwest::Error> =
        reqwest::get(&format!("{}/hosts", &app.address)).await;
    assert_eq!(res.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn test_add_host() {
    let app = spawn_app().await;
    let host = NewHost {
        name: String::from("test_host"),
        address: String::from("127.0.0.1"),
        port: 8080,
        host_user: String::from("root"),
        password: String::from("pass"),
    };
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/hosts", &app.address))
        .header("Content-Type", "application/json")
        .json(&host)
        .send()
        .await;
    assert_eq!(res.unwrap().status(), StatusCode::CREATED);
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let db_name = self.db_name.clone();

        std::thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let config = get_configuration().expect("Failed to read configuration");
                let mut conn = PgConnection::connect_with(&config.database.without_db())
                    .await
                    .expect("Failed to connect to Postgres");

                conn.execute(&*format!("DROP DATABASE \"{}\" WITH (FORCE)", db_name))
                    .await
                    .expect("Failed to drop database.");

                tracing::info!("Dropped database: {db_name}");
                let _ = tx.send(());
            })
        });

        let _ = rx.recv();
    }
}
