use std::time::Duration;

use tonic::transport::{server::Router, Server};
use tracing::Span;

use crate::{rpc::node::vm_service_server::VmServiceServer, services::vm::service::VmmService};

pub async fn run() -> Result<Router, Box<dyn std::error::Error>> {
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<VmServiceServer<VmmService>>()
        .await;

    let server = Server::builder()
        .trace_fn(|_| Span::current())
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .add_service(health_service)
        .add_service(VmServiceServer::new(VmmService::new()));

    Ok(server)
}
