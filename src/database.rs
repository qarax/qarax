use sqlx::{
    migrate::MigrateDatabase,
    postgres::{self, PgPoolOptions},
    sqlx_macros::migrate,
    PgPool,
};

pub async fn connect(db_url: &str) -> anyhow::Result<PgPool> {
    let max_connections = &dotenv::var("MAX_CONNECTIONS")
        .unwrap_or(String::from("10"))
        .parse()
        .unwrap();

    let pool = PgPoolOptions::new()
        .max_connections(*max_connections)
        .connect(db_url)
        .await?;

    Ok(pool)
}

pub async fn run_migrations(db_url: &str) -> anyhow::Result<()> {
    tracing::info!("Checking if database exits...");
    if !postgres::Postgres::database_exists(db_url).await? {
        tracing::info!("Creating database...");
        postgres::Postgres::create_database(db_url).await?;
    }

    let pool = PgPool::connect(db_url).await?;
    tracing::info!("Run migrations...");
    migrate!("./migrations").run(&pool).await?;
    pool.close().await;

    Ok(())
}
