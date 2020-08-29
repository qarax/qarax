use crate::create_backend;
use crate::database::DbConnection;

use rocket::local::Client;

embed_migrations!();

#[allow(dead_code)]
pub fn get_client(mount: &str, routes: Vec<rocket::Route>) -> (Client, DbConnection) {
    let rocket = rocket::ignite()
        .manage(create_backend())
        .attach(DbConnection::fairing())
        .mount(mount, routes);

    let conn = DbConnection::get_one(&rocket).expect("Database connection failed");
    embedded_migrations::run(&*conn).expect("Failed to run migrations");
    let client = Client::new(rocket).expect("Failed to get client");
    (client, conn)
}
