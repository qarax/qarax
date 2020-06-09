#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;
#[macro_use]
extern crate diesel;

mod controllers;
mod database;
mod models;
mod schema;
mod services;

use controllers::hosts;

fn main() {
    rocket::ignite()
        .attach(database::Connection::fairing())
        .mount("/hosts", routes![hosts::index, hosts::add_host])
        .launch();
}
