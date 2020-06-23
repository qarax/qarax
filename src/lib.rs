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

use controllers::hosts;
use rocket::Rocket;

use database::DbConnection;
use services::host::HostService;
use services::vm::VmService;
use services::Backend;

pub fn rocket() -> Rocket {
    rocket::ignite()
        .attach(DbConnection::fairing())
        .manage(Backend {
            host_service: HostService::new(),
            vm_service: VmService::new(),
        })
        .mount("/hosts", hosts::routes())
}
