use anyhow::{Context, anyhow};
use serde::{Serialize, de::DeserializeOwned};

pub struct Client {
    inner: reqwest::Client,
    base_url: String,
}

impl Client {
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
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

    /// DELETE request, discarding the response body.
    pub async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let resp = self
            .inner
            .delete(self.url(path))
            .send()
            .await
            .with_context(|| format!("DELETE {path}"))?;
        Self::check_error(resp).await?;
        Ok(())
    }
}
