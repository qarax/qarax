use std::error;
use std::io::{Error, ErrorKind};
use hyper::{Body, Client, Request};
use hyperlocal::{Uri, UnixClientExt, UnixConnector};
use futures::stream::{Stream, TryStreamExt};

const HTTP_VERSION: &str = "HTTP/1.1";

type Result<T> = std::result::Result<T, Error>; 

pub enum Method {
    GET,
    PUT,
    PATCH
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

pub struct VmmClient<'a> {
    client: Client<UnixConnector>,
    socket_path: &'a str,
}

impl<'a> VmmClient<'a> {
    pub fn new(socket_path: &'a str) -> Self {
        VmmClient {
            client: Client::unix(),
            socket_path,
        }
    }

    pub async fn request(&self, endpoint: &'a str,  method: Method, body: &'a [u8]) -> Result<String> {
        //let uri = Uri::new("/tmp/firecracker.sock", "/");
        println!("socket path: {} endpoint {}", self.socket_path, endpoint);

        let req = Request::builder()
            .method(method.as_str())
            .uri(Uri::new(self.socket_path, endpoint))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_vec()))
            .unwrap();
    
        let resp = self.client.request(req).await;
        let resp = match resp {
            Ok(response) => {
                response
            },
            Err(e) => return Err(Error::new(ErrorKind::Other, e.to_string())),
        };
        println!("Incoming status: {}", resp.status());

        let bytes = resp.into_body()
        .try_fold(Vec::default(), |mut buf, bytes| async {
            buf.extend(bytes);
            Ok(buf)
        })
        .await.unwrap();

        println!("string {:?}", String::from_utf8(bytes.to_vec()));
        Ok(String::from_utf8(bytes).expect("Couldn't convert to string"))
    }
}