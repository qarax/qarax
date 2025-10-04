use sqlx::{
    PgPool,
    migrate::MigrateDatabase,
    postgres::{self},
};

pub async fn run_migrations(db_url: &str) -> Result<(), sqlx::Error> {
    tracing::info!("Checking if database exits...");
    if !postgres::Postgres::database_exists(db_url).await? {
        tracing::info!("Database does not exist, creating...");
        postgres::Postgres::create_database(db_url).await?;
    }

    let conn = PgPool::connect(db_url).await?;
    sqlx::migrate!("../migrations").run(&conn).await?;

    Ok(())
}
