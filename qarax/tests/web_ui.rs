mod support;

use reqwest::{StatusCode, header::CACHE_CONTROL};
use support::{UI_INDEX, spawn_app};

/// Every page of the web UI is its index.html, which browsers must revalidate
/// on each load: it names the build's hashed files, and a stale copy would load
/// the previous release's. The hashed files never change; one the build
/// doesn't have is a 404, not the page served as a script.
#[tokio::test]
async fn test_web_ui_pages_and_caching() {
    let app = spawn_app().await;
    let get = |path: &str| reqwest::get(format!("{}{path}", app.root));

    for path in ["/ui", "/ui/", "/ui/vms/some-id"] {
        let page = get(path).await.unwrap();
        assert_eq!(page.status(), StatusCode::OK, "{path}");
        assert_eq!(page.headers()[CACHE_CONTROL], "no-cache", "{path}");
        assert_eq!(page.text().await.unwrap(), UI_INDEX, "{path}");
    }

    let asset = get("/ui/_app/immutable/app.js").await.unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    let missing = get("/ui/_app/immutable/gone.js").await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(missing.headers()[CACHE_CONTROL], "no-cache");
}
