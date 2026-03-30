use std::future::IntoFuture;
use tokio::net::TcpListener;

use sqlx::PgPool;

use crate::{
    App,
    configuration::{SchedulingSettings, VmDefaultsSettings},
    handlers::app,
};

pub async fn run(
    listener: TcpListener,
    db_pool: PgPool,
    vm_defaults: VmDefaultsSettings,
    scheduling: SchedulingSettings,
    control_plane_architecture: String,
) -> Result<impl IntoFuture<Output = std::io::Result<()>> + Send, Box<dyn std::error::Error + Send>>
{
    crate::model::events::init_event_bus();

    let a = App::new(db_pool, vm_defaults, scheduling, control_plane_architecture);

    // Spawn background task to reconcile VM status with the live node state
    tokio::spawn(crate::vm_monitor::start_vm_monitor(a.pool_arc()));

    // Spawn background task to poll host resource metrics
    tokio::spawn(crate::resource_monitor::start_resource_monitor(
        a.pool_arc(),
    ));

    // Spawn background task to deliver lifecycle hook webhooks
    tokio::spawn(crate::hook_executor::start_hook_executor(a.pool_arc()));

    // Spawn background task to reap idle sandboxes
    tokio::spawn(crate::sandbox_reaper::start_sandbox_reaper(a.pool_arc()));

    let app = app(a);
    let server = axum::serve(listener, app);
    Ok(server)
}
