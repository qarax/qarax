use diesel::PgConnection;

#[cfg_attr(test, database("qarax_db_test"))]
#[cfg_attr(not(test), database("qarax_db"))]
pub struct DbConnection(PgConnection);
