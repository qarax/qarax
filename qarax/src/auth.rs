use axum::{
    body::Body,
    extract::State,
    http::{Request, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{App, errors::Error};

/// Paths that remain reachable without an API token (health check and docs).
fn is_public_path(path: &str) -> bool {
    path == "/" || path == "/api-docs/openapi.json" || path.starts_with("/swagger-ui")
}

pub async fn require_api_token(
    State(env): State<App>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let auth = env.auth();
    if !auth.enabled || is_public_path(request.uri().path()) {
        return next.run(request).await;
    }

    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match token {
        Some(token) if auth.tokens.iter().any(|t| t == token) => next.run(request).await,
        _ => Error::Unauthorized.into_response(),
    }
}
