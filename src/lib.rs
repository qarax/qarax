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
use services::drive::DriveService;
use services::host::HostService;
use services::kernel::KernelService;
use services::storage::StorageService;
use services::vm::VmService;
use services::Backend;

embed_migrations!();

pub fn rocket() -> Rocket {
    rocket::ignite()
        .attach(DbConnection::fairing())
        .attach(AdHoc::on_launch("Run migrations", |rocket| {
            let connection: DbConnection = DbConnection::get_one(rocket).unwrap();
            embedded_migrations::run(&*connection).expect("Database connection failed");
        }))
        .manage(Backend {
            host_service: HostService::new(),
            vm_service: VmService::new(),
            storage_service: StorageService::new(),
            drive_service: DriveService::new(),
            kernel_service: KernelService::new(),
        })
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
