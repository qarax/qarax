use validator::ValidationErrors;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an internal database error occurred")]
    Sqlx(sqlx::Error),

    #[error("validation error in request body")]
    InvalidEntity(#[from] ValidationErrors),

    #[error("{0}")]
    UnprocessableEntity(String),

    #[error("{0}")]
    Conflict(String),

    #[error("internal server error")]
    InternalServerError,

    #[error("not found")]
    NotFound,
}

fn unique_violation_message(db_err: &dyn sqlx::error::DatabaseError) -> String {
    // Constraint names follow the pattern `{table}_{column}_key`.
    // Parse them to produce a human-friendly message.
    if let Some(constraint) = db_err.constraint() {
        let parts: Vec<&str> = constraint.splitn(3, '_').collect();
        if parts.len() >= 2 {
            let resource = parts[0];
            // Strip trailing "_key" suffix to get the column name(s).
            let field = constraint
                .strip_prefix(&format!("{}_", resource))
                .and_then(|s| s.strip_suffix("_key"))
                .unwrap_or(parts[1]);
            return format!(
                "{} with this {} already exists",
                to_singular_title(resource),
                field
            );
        }
    }
    "resource with this value already exists".to_string()
}

fn to_singular_title(s: &str) -> String {
    let singular = s.strip_suffix('s').unwrap_or(s);
    let mut c = singular.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Error::NotFound,
            sqlx::Error::Database(ref db_err) if db_err.code().as_deref() == Some("23505") => {
                let msg = unique_violation_message(db_err.as_ref());
                Error::Conflict(msg)
            }
            _ => {
                tracing::error!(error = %err, error.debug = ?err, "database error");
                Error::Sqlx(err)
            }
        }
    }
}
