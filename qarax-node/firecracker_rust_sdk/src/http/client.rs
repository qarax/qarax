use super::*;
use hyper::{Body, Client, Request};
use hyperlocal::{UnixClientExt, UnixConnector, Uri};

pub enum Method {
    GET,
    PUT,
    PATCH,
}

impl Method {
    pub fn as_str(&self) -> &str {
        match &self {
            Method::GET => "GET",
            Method::PUT => "PUT",
            Method::PATCH => "PATCH",
        }
    }
}

#[derive(Debug)]
pub struct VmmClient {
    client: Client<UnixConnector>,
    pub socket_path: String,
}

impl VmmClient {
    pub fn new(socket_path: String) -> Self {
        VmmClient {
            client: Client::unix(),
            socket_path,
        }
    }

    pub async fn request(&self, endpoint: &str, method: Method, body: &[u8]) -> Result<String> {
        let req = Request::builder()
            .method(method.as_str())
            .uri(Uri::new(&self.socket_path, endpoint))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_vec()))?;

        let mut resp = self.client.request(req).await?;
        tracing::debug!("Incoming status: {}", resp.status());

        let bytes = hyper::body::to_bytes(resp.body_mut()).await?;

        Ok(String::from_utf8(bytes.to_vec()).expect("Couldn't convert to string"))
    }
}
