use anyhow::{Context, anyhow};
use serde::{Serialize, de::DeserializeOwned};

use crate::api::models::Page;

/// Items per request when following a list's pages: the most the API serves.
const PAGE_LIMIT: usize = 1000;

pub struct Client {
    inner: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl Client {
    pub fn new(base_url: &str, token: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder();
        if let Some(token) = token {
            let mut headers = reqwest::header::HeaderMap::new();
            let mut auth_value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("API token must be a valid header value");
            auth_value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, auth_value);
            builder = builder.default_headers(headers);
        }
        Self {
            inner: builder.build().expect("failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(str::to_owned),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The API token, for connections made outside this HTTP client (the
    /// console WebSocket).
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// `path` on the API, which the server serves under `/v1`.
    fn url(&self, path: &str) -> String {
        format!("{}/v1{}", self.base_url, path)
    }

    /// Checks that the response status is 2xx; on error, extracts the message
    /// from the response body and returns it as an error. On success, returns
    /// the response unconsumed so the caller can read the body.
    pub async fn check_error(resp: reqwest::Response) -> anyhow::Result<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let _status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
            && let Some(msg) = json.get("message").and_then(|v| v.as_str())
        {
            return Err(anyhow!("{}", msg));
        }
        Err(anyhow!("{}", body.trim()))
    }

    /// GET request, deserializing the response body as JSON.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self
            .inner
            .get(self.url(path))
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        Self::check_error(resp)
            .await?
            .json()
            .await
            .context("failed to parse response")
    }

    /// GET every item of the paginated list at `path` (which may have a
    /// query string), following `next_cursor` to the last page.
    pub async fn get_list<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<Vec<T>> {
        self.get_list_up_to(path, usize::MAX).await
    }

    /// GET the first `max` items of the paginated list at `path`, in the
    /// list's order, following `next_cursor` until there are that many.
    pub async fn get_list_up_to<T: DeserializeOwned>(
        &self,
        path: &str,
        max: usize,
    ) -> anyhow::Result<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        while items.len() < max {
            let mut query = vec![("limit", PAGE_LIMIT.min(max - items.len()).to_string())];
            if let Some(cursor) = cursor.take() {
                query.push(("cursor", cursor));
            }
            let resp = self
                .inner
                .get(self.url(path))
                .query(&query)
                .send()
                .await
                .with_context(|| format!("GET {path}"))?;
            let page: Page<T> = Self::check_error(resp)
                .await?
                .json()
                .await
                .context("failed to parse response")?;
            items.extend(page.items);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(items)
    }

    /// GET request, returning the response body as plain text.
    pub async fn get_text(&self, path: &str) -> anyhow::Result<String> {
        let resp = self
            .inner
            .get(self.url(path))
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        Self::check_error(resp)
            .await?
            .text()
            .await
            .context("failed to read response body")
    }

    /// POST request with a JSON body; returns the raw (unchecked) response so
    /// callers can inspect the status code before parsing the body.
    pub async fn post_response<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<reqwest::Response> {
        let resp = self
            .inner
            .post(self.url(path))
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        Self::check_error(resp).await
    }

    /// POST request with a JSON body, deserializing the response as JSON.
    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        self.post_response(path, body)
            .await?
            .json()
            .await
            .context("failed to parse response")
    }

    /// POST request with a JSON body, returning the response body as plain text.
    pub async fn post_text<B: Serialize>(&self, path: &str, body: &B) -> anyhow::Result<String> {
        self.post_response(path, body)
            .await?
            .text()
            .await
            .context("failed to read response body")
    }

    /// POST request with an empty body, discarding the response.
    pub async fn post_empty(&self, path: &str) -> anyhow::Result<()> {
        let resp = self
            .inner
            .post(self.url(path))
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        Self::check_error(resp).await?;
        Ok(())
    }

    /// POST request with an empty body, deserializing the response as JSON.
    pub async fn post_empty_json<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self
            .inner
            .post(self.url(path))
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        Self::check_error(resp)
            .await?
            .json()
            .await
            .context("failed to parse response")
    }

    /// PUT request with a JSON body, deserializing the response as JSON.
    pub async fn put<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        let resp = self
            .inner
            .put(self.url(path))
            .json(body)
            .send()
            .await
            .with_context(|| format!("PUT {path}"))?;
        Self::check_error(resp)
            .await?
            .json()
            .await
            .context("failed to parse response")
    }

    /// PATCH request with a JSON body, deserializing the response as JSON.
    pub async fn patch<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        let resp = self
            .inner
            .patch(self.url(path))
            .json(body)
            .send()
            .await
            .with_context(|| format!("PATCH {path}"))?;
        Self::check_error(resp)
            .await?
            .json()
            .await
            .context("failed to parse response")
    }

    /// DELETE request, discarding the response body.
    pub async fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.delete_status(path).await?;
        Ok(())
    }

    /// DELETE request retaining the status for asynchronous operations.
    pub async fn delete_status(&self, path: &str) -> anyhow::Result<reqwest::StatusCode> {
        let resp = self
            .inner
            .delete(self.url(path))
            .send()
            .await
            .with_context(|| format!("DELETE {path}"))?;
        Ok(Self::check_error(resp).await?.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Accepts a single connection, captures the raw request, and replies
    /// with an empty 200 response.
    async fn capture_request(listener: TcpListener) -> String {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = socket.read(&mut buf).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8_lossy(&buf[..n]).to_lowercase()
    }

    #[tokio::test]
    async fn sets_authorization_header_when_token_provided() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(capture_request(listener));

        let client = Client::new(&format!("http://{addr}"), Some("my-token"));
        let _ = client.get_text("/").await;

        let request = server.await.unwrap();
        assert!(request.starts_with("get /v1/ "), "{request}");
        assert!(request.contains("authorization: bearer my-token"));
    }

    #[tokio::test]
    async fn get_list_follows_next_cursor_to_the_last_page() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut request_lines = Vec::new();
            for body in [
                r#"{"items":[1,2],"next_cursor":"abc"}"#,
                r#"{"items":[3],"next_cursor":null}"#,
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                request_lines.push(request.lines().next().unwrap().to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            request_lines
        });

        let client = Client::new(&format!("http://{addr}"), None);
        let items: Vec<u32> = client.get_list("/vms?name=x").await.unwrap();

        assert_eq!(items, [1, 2, 3]);
        assert_eq!(
            server.await.unwrap(),
            [
                "GET /v1/vms?name=x&limit=1000 HTTP/1.1",
                "GET /v1/vms?name=x&limit=1000&cursor=abc HTTP/1.1",
            ]
        );
    }

    #[tokio::test]
    async fn no_authorization_header_when_token_absent() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(capture_request(listener));

        let client = Client::new(&format!("http://{addr}"), None);
        let _ = client.get_text("/").await;

        let request = server.await.unwrap();
        assert!(!request.contains("authorization:"));
    }
}
