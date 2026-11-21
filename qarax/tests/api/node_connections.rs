//! The control plane's connections to the nodes (`qarax::grpc_client`):
//! clients share one per node, and losing it fails calls as connecting
//! afresh would. Against a fake node that serves with the nodes' TLS.

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use qarax::grpc_client::NodeClient;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::support::{init_node_tls, node_server_tls};

/// A node whose every VmService call succeeds at once with an empty message,
/// which reads as the response's defaults.
#[derive(Clone)]
struct AnsweringNode;

impl tonic::server::NamedService for AnsweringNode {
    const NAME: &'static str = "node.VmService";
}

impl tower::Service<http::Request<tonic::body::BoxBody>> for AnsweringNode {
    type Response = http::Response<tonic::body::BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<tonic::body::BoxBody>) -> Self::Future {
        Box::pin(async move {
            let mut grpc = tonic::server::Grpc::new(tonic::codec::ProstCodec::<(), ()>::default());
            Ok(grpc.unary(Empty, request).await)
        })
    }
}

struct Empty;

impl tonic::server::UnaryService<()> for Empty {
    type Response = ();
    type Future = std::future::Ready<Result<tonic::Response<()>, tonic::Status>>;

    fn call(&mut self, _: tonic::Request<()>) -> Self::Future {
        std::future::ready(Ok(tonic::Response::new(())))
    }
}

/// An [`AnsweringNode`] serving until stopped, counting the TCP connections
/// it accepts.
struct FakeNode {
    accepted: Arc<AtomicUsize>,
    stop: oneshot::Sender<()>,
    server: JoinHandle<()>,
}

impl FakeNode {
    fn serve(listener: TcpListener) -> Self {
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        let incoming = async_stream::stream! {
            loop {
                let connection = listener.accept().await.map(|(stream, _)| stream);
                counter.fetch_add(1, Ordering::SeqCst);
                yield connection;
            }
        };
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(node_server_tls())
                .unwrap()
                .add_service(AnsweringNode)
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            accepted,
            stop,
            server,
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Shuts down as qarax-node does on SIGTERM: closes its connections once
    /// their calls are done, then stops listening.
    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.server.await.unwrap();
    }
}

async fn listen() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// A client for one operation, as the handlers and monitors make them.
fn client(port: u16) -> NodeClient {
    NodeClient::new("127.0.0.1", port)
}

#[tokio::test]
async fn node_calls_share_one_connection() {
    init_node_tls();
    let (listener, port) = listen().await;
    let node = FakeNode::serve(listener);

    client(port).get_node_info().await.unwrap();
    client(port).start_vm(Uuid::new_v4()).await.unwrap();

    assert_eq!(node.accepted(), 1);
}

#[tokio::test]
async fn calls_to_a_restarting_node_fail_and_retry_as_connecting_does() {
    init_node_tls();
    let (listener, port) = listen().await;
    let node = FakeNode::serve(listener);
    client(port).get_node_info().await.unwrap();

    node.stop().await;
    // Lets the control plane read that the node closed the connection, which
    // it does long before its next call to a node that restarts.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let error = format!("{:#}", client(port).get_node_info().await.unwrap_err());
    assert!(
        error.starts_with("Failed to connect to qarax-node: ")
            && error.contains("Connection refused"),
        "{error}"
    );

    // start_vm retries a refused connection once a second, long enough for
    // the node to come back.
    let restart = async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        FakeNode::serve(TcpListener::bind(("127.0.0.1", port)).await.unwrap())
    };
    let starter = client(port);
    let (started, _node) = tokio::join!(starter.start_vm(Uuid::new_v4()), restart);
    started.unwrap();
    client(port).get_node_info().await.unwrap();
}

/// Forwards each connection on `listener` to the node on `node_port` until
/// `frozen`. Then it stops forwarding but keeps the connections open, as a
/// host that lost power would (no FIN, no RST), and refuses new ones (where
/// that host wouldn't answer them at all, which
/// `connecting_to_an_unreachable_node_fails_fast` covers).
async fn freezable_proxy(listener: TcpListener, node_port: u16, mut frozen: watch::Receiver<bool>) {
    loop {
        let mut connection = tokio::select! {
            accepted = listener.accept() => accepted.unwrap().0,
            _ = frozen.wait_for(|frozen| *frozen) => return,
        };
        let mut node = TcpStream::connect(("127.0.0.1", node_port)).await.unwrap();
        let mut connection_frozen = frozen.clone();
        tokio::spawn(async move {
            let froze = tokio::select! {
                _ = tokio::io::copy_bidirectional(&mut connection, &mut node) => false,
                _ = connection_frozen.wait_for(|frozen| *frozen) => true,
            };
            if froze {
                std::future::pending::<()>().await;
            }
        });
    }
}

#[tokio::test]
async fn a_call_to_a_node_that_stopped_answering_fails_within_the_keepalive() {
    init_node_tls();
    let (listener, node_port) = listen().await;
    let _node = FakeNode::serve(listener);
    let (proxy, port) = listen().await;
    let (freeze, frozen) = watch::channel(false);
    tokio::spawn(freezable_proxy(proxy, node_port, frozen));
    client(port).get_node_info().await.unwrap();

    freeze.send(true).unwrap();
    // The keepalive pings after 5 s without a frame from the node and gives
    // up 10 s later; a second to spare.
    let call = tokio::time::timeout(Duration::from_secs(16), client(port).get_node_info())
        .await
        .expect("the call outlived the keepalive");
    assert!(call.is_err());

    // The connection is gone, so the next call connects afresh.
    let error = format!("{:#}", client(port).get_node_info().await.unwrap_err());
    assert!(
        error.starts_with("Failed to connect to qarax-node: ")
            && error.contains("Connection refused"),
        "{error}"
    );
}
