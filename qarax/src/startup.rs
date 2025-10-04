use axum::{Router, routing::IntoMakeService, serve::Serve};
use tokio::net::TcpListener;

use sqlx::PgPool;

use crate::{App, handlers::app};

pub async fn run(
    listener: TcpListener,
    db_pool: PgPool,
) -> Result<Serve<IntoMakeService<Router>, Router>, Box<dyn std::error::Error + Send>> {
    let a = App::new(db_pool);
    let app = app(a);
    let server = axum::serve(listener, app.into_make_service());
    Ok(server)
}
