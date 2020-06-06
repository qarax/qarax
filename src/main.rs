#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate diesel;

mod database;
mod schema;
mod models;
mod controllers;

use controllers::hosts;

fn main() {
    rocket::ignite()
    .attach(database::Connection::fairing())
    .mount("/", routes![hosts::index])
    .launch();
}
