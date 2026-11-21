use sqlx::{
    PgPool,
    migrate::MigrateDatabase,
    postgres::{self, PgPoolOptions},
};

use crate::configuration::DatabaseSettings;

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

/// The control plane's pool on the configured database: at most
/// `max_connections` connections, opened as queries need them.
pub fn connect_lazy(settings: &DatabaseSettings) -> PgPool {
    PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .connect_lazy_with(settings.with_db())
}

#[cfg(test)]
mod tests {
    use super::connect_lazy;
    use crate::test_support::TestDatabase;

    #[tokio::test]
    async fn the_pool_is_on_the_configured_database_with_the_configured_size() {
        let db = TestDatabase::new().await;
        let mut settings = db.app().database().clone();
        settings.max_connections = 3;

        let pool = connect_lazy(&settings);

        let database: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(database, settings.name);
        assert_eq!(pool.options().get_max_connections(), 3);
        pool.close().await;
    }
}
