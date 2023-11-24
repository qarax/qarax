use std::net::TcpListener;

use axum::{routing::IntoMakeService, Router, Server};
use hyper::server::conn::AddrIncoming;
use sqlx::PgPool;

use crate::{handlers::app, App};

pub fn run(
    listener: TcpListener,
    db_pool: PgPool,
) -> Result<Server<AddrIncoming, IntoMakeService<Router>>, Box<dyn std::error::Error>> {
    let a = App::new(db_pool);
    let app = app(a);
    let server = axum::Server::from_tcp(listener)?.serve(app.into_make_service());
    Ok(server)
}
