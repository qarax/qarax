use axum::{
    body::Body,
    extract::State,
    http::{
        HeaderMap, Request,
        header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{App, errors::Error};

/// The prefix of the WebSocket subprotocol that carries an API token,
/// hex-encoded, for clients that can't set the Authorization header on an
/// upgrade request, as browsers can't: `qarax.bearer.<hex of the token>`.
/// Hex because a subprotocol name allows only some ASCII characters.
pub const TOKEN_PROTOCOL_PREFIX: &str = "qarax.bearer.";

/// Paths that remain reachable without an API token: the health check, the
/// API docs and the web UI's static files (the UI asks for a token itself).
fn is_public_path(path: &str) -> bool {
    path == "/"
        || path == "/api-docs/openapi.json"
        || path.starts_with("/swagger-ui")
        || path == "/ui"
        || path.starts_with("/ui/")
}

/// The request's API token: from the Authorization header or, for a browser's
/// WebSocket upgrade, from its [`TOKEN_PROTOCOL_PREFIX`] subprotocol.
fn request_token(headers: &HeaderMap) -> Option<String> {
    let bearer = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    bearer.or_else(|| {
        headers
            .get_all(SEC_WEBSOCKET_PROTOCOL)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .find_map(|protocol| protocol.trim().strip_prefix(TOKEN_PROTOCOL_PREFIX))
            .and_then(|encoded| hex::decode(encoded).ok())
            .and_then(|token| String::from_utf8(token).ok())
    })
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

    match request_token(request.headers()) {
        Some(token) if auth.tokens.contains(&token) => next.run(request).await,
        _ => Error::Unauthorized.into_response(),
    }
}
