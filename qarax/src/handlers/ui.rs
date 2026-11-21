//! The web UI (`webui/`, a single-page app) at `/ui`, served from its static
//! build in the configured `ui_dir`.

use std::path::Path;

use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use http::{HeaderValue, header::CACHE_CONTROL};
use tower_http::services::{ServeDir, ServeFile};

pub fn router(dir: &Path) -> Router {
    // A path that isn't a file is one of the app's pages, which index.html
    // routes itself; but not under _app/, where the build's scripts and styles
    // are: a missing one there is a 404, not the page served as a script.
    let index = ServeFile::new(dir.join("index.html"))
        .precompressed_br()
        .precompressed_gzip();
    let pages = ServeDir::new(dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(index);
    let assets = ServeDir::new(dir.join("_app"))
        .precompressed_br()
        .precompressed_gzip();
    Router::new()
        .nest_service("/ui/_app", assets)
        .nest_service("/ui", pages)
        .layer(middleware::from_fn(cache_control))
}

/// Files under `_app/immutable/` have their content's hash in their names, so
/// browsers may keep them for good. Everything else is revalidated on each
/// load, index.html above all: it names the current hashes, and a stale copy
/// would load the previous release's files.
async fn cache_control(request: Request, next: Next) -> Response {
    let immutable = request.uri().path().starts_with("/ui/_app/immutable/");
    let mut response = next.run(request).await;
    let value = if immutable && response.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(value));
    response
}
