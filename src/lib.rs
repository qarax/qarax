#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod common;
mod controllers;
mod database;
mod models;
mod schema;
mod services;

use rocket::fairing::AdHoc;
use rocket::Rocket;
use rocket::State;

use controllers::drives;
use controllers::hosts;
use controllers::kernels;
use controllers::storage;
use controllers::vms;
use database::DbConnection;

pub use services::drive::DriveService;
pub use services::host::HostService;
pub use services::kernel::KernelService;
pub use services::storage::StorageService;
pub use services::vm::VmService;
pub use services::Backend;

embed_migrations!();

pub fn rocket(backend: Backend) -> Rocket {
    rocket::ignite()
        .attach(DbConnection::fairing())
        .attach(AdHoc::on_launch("Run migrations", |rocket| {
            let connection: DbConnection = DbConnection::get_one(rocket).unwrap();
            embedded_migrations::run(&*connection).expect("Database connection failed");
        }))
        .manage(backend)
        .attach(AdHoc::on_launch("Initialize hosts", |rocket| {
            let backend: State<Backend> = State::from(rocket).unwrap();
            backend
                .host_service
                .initialize_hosts(&DbConnection::get_one(&rocket).unwrap())
        }))
        .mount("/hosts", hosts::routes())
        .mount("/vms", vms::routes())
        .mount("/storage", storage::routes())
        .mount("/drives", drives::routes())
        .mount("/kernels", kernels::routes())
}
