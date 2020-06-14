/*
 * Firecracker API
 *
 * RESTful public-facing API. The API is accessible through HTTP calls on specific URLs carrying JSON modeled data. The transport medium is a Unix Domain Socket.
 *
 * The version of the OpenAPI document: 0.21.0
 * Contact: compute-capsule@amazon.com
 * Generated by: https://openapi-generator.tech
 */

/// Vsock : Defines a vsock device, backed by a set of Unix Domain Sockets, on the host side. For host-initiated connections, Firecracker will be listening on the Unix socket identified by the path `uds_path`. Firecracker will create this socket, bind and listen on it. Host-initiated connections will be performed by connection to this socket and issuing a connection forwarding request to the desired guest-side vsock port (i.e. `CONNECT 52\\n`, to connect to port 52). For guest-initiated connections, Firecracker will expect host software to be bound and listening on Unix sockets at `uds_path_<PORT>`. E.g. \"/path/to/host_vsock.sock_52\" for port number 52.



#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vsock {
    /// Guest Vsock CID
    #[serde(rename = "guest_cid")]
    pub guest_cid: i32,
    /// Path to UNIX domain socket, used to proxy vsock connections.
    #[serde(rename = "uds_path")]
    pub uds_path: String,
    #[serde(rename = "vsock_id")]
    pub vsock_id: String,
}

impl Vsock {
    /// Defines a vsock device, backed by a set of Unix Domain Sockets, on the host side. For host-initiated connections, Firecracker will be listening on the Unix socket identified by the path `uds_path`. Firecracker will create this socket, bind and listen on it. Host-initiated connections will be performed by connection to this socket and issuing a connection forwarding request to the desired guest-side vsock port (i.e. `CONNECT 52\\n`, to connect to port 52). For guest-initiated connections, Firecracker will expect host software to be bound and listening on Unix sockets at `uds_path_<PORT>`. E.g. \"/path/to/host_vsock.sock_52\" for port number 52.
    pub fn new(guest_cid: i32, uds_path: String, vsock_id: String) -> Vsock {
        Vsock {
            guest_cid,
            uds_path,
            vsock_id,
        }
    }
}


