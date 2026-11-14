//! A DNS forwarder on each isolated network's gateway address.
//!
//! Guests are told to use their gateway as DNS server (DHCP and the kernel
//! `ip=` line do, unless the network names its own), and on an isolated
//! network the gateway is this host's bridge. The forwarder relays their
//! queries, over UDP and TCP, to the resolvers in the host's
//! /etc/resolv.conf, so guest names resolve the way the host's do. Its sockets
//! are bound to the bridge, so only that network's guests can use it.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{debug, info, warn};

const DNS_PORT: u16 = 53;
const RESOLV_CONF: &str = "/etc/resolv.conf";
/// How long to wait for one upstream resolver before trying the next.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a TCP query (a truncated UDP answer, retried) may take end to end.
const TCP_SESSION_TIMEOUT: Duration = Duration::from_secs(10);

static FORWARDERS: LazyLock<Mutex<HashMap<String, Vec<JoinHandle<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Start (or restart) the forwarder for a bridge, listening on `listen`, the
/// bridge's gateway address.
pub async fn start_dns_forwarder(bridge: &str, listen: Ipv4Addr) -> Result<()> {
    info!("Starting DNS forwarder on {bridge} ({listen}:{DNS_PORT})");
    // A re-attach restarts the forwarder: the old one's listener must be closed
    // before the port can be bound again.
    stop_dns_forwarder(bridge).await;
    let addr = SocketAddr::from((listen, DNS_PORT));
    let udp = bind(bridge, addr, socket2::Type::DGRAM)?;
    let udp = Arc::new(UdpSocket::from_std(udp.into()).context("Failed to wrap UDP socket")?);
    let tcp = bind(bridge, addr, socket2::Type::STREAM)?;
    tcp.listen(128)
        .context("Failed to listen for DNS over TCP")?;
    let tcp = TcpListener::from_std(tcp.into()).context("Failed to wrap TCP listener")?;

    let name = bridge.to_string();
    let handles = vec![
        tokio::spawn(serve_udp(udp, name.clone())),
        tokio::spawn(serve_tcp(tcp, name)),
    ];
    FORWARDERS
        .lock()
        .unwrap()
        .insert(bridge.to_string(), handles);
    Ok(())
}

/// Stop the forwarder for a bridge, returning once its sockets are closed.
pub async fn stop_dns_forwarder(bridge: &str) {
    let Some(handles) = FORWARDERS.lock().unwrap().remove(bridge) else {
        return;
    };
    info!("Stopping DNS forwarder for bridge {bridge}");
    for handle in handles {
        handle.abort();
        // Cancelled, as asked: the task and its socket are gone.
        let _ = handle.await;
    }
}

fn bind(bridge: &str, addr: SocketAddr, kind: socket2::Type) -> Result<socket2::Socket> {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, kind, None)
        .context("Failed to create DNS socket")?;
    // A restart rebinds while the old task's socket may still be open.
    socket
        .set_reuse_address(true)
        .context("Failed to set SO_REUSEADDR")?;
    socket
        .bind_device(Some(bridge.as_bytes()))
        .context("Failed to bind DNS socket to the bridge")?;
    socket
        .bind(&addr.into())
        .with_context(|| format!("Failed to bind DNS socket to {addr}"))?;
    socket
        .set_nonblocking(true)
        .context("Failed to set DNS socket non-blocking")?;
    Ok(socket)
}

/// The nameservers in a resolv.conf, in order.
fn nameservers(resolv_conf: &str) -> Vec<IpAddr> {
    resolv_conf
        .lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .collect()
}

/// The host's resolvers, read per query so changes to resolv.conf apply.
async fn upstreams() -> Vec<SocketAddr> {
    match tokio::fs::read_to_string(RESOLV_CONF).await {
        Ok(contents) => nameservers(&contents)
            .into_iter()
            .map(|ip| SocketAddr::new(ip, DNS_PORT))
            .collect(),
        Err(e) => {
            warn!("Failed to read {RESOLV_CONF}: {e}");
            Vec::new()
        }
    }
}

async fn serve_udp(socket: Arc<UdpSocket>, bridge: String) {
    let mut buf = [0u8; 4096];
    loop {
        let (len, client) = match socket.recv_from(&mut buf).await {
            Ok(received) => received,
            Err(e) => {
                warn!("DNS forwarder on {bridge} stopped receiving: {e}");
                return;
            }
        };
        let query = buf[..len].to_vec();
        let socket = socket.clone();
        tokio::spawn(async move {
            match forward_udp(&query).await {
                Some(answer) => {
                    if let Err(e) = socket.send_to(&answer, client).await {
                        debug!("Failed to return a DNS answer to {client}: {e}");
                    }
                }
                None => debug!("No resolver answered a query from {client}"),
            }
        });
    }
}

async fn forward_udp(query: &[u8]) -> Option<Vec<u8>> {
    for upstream in upstreams().await {
        let local: SocketAddr = if upstream.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let Ok(socket) = UdpSocket::bind(local).await else {
            continue;
        };
        if socket.connect(upstream).await.is_err() || socket.send(query).await.is_err() {
            continue;
        }
        let mut buf = vec![0u8; 65535];
        if let Ok(Ok(len)) = timeout(UPSTREAM_TIMEOUT, socket.recv(&mut buf)).await {
            buf.truncate(len);
            return Some(buf);
        }
    }
    None
}

async fn serve_tcp(listener: TcpListener, bridge: String) {
    loop {
        let (mut client, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                warn!("DNS forwarder on {bridge} stopped accepting: {e}");
                return;
            }
        };
        tokio::spawn(async move {
            for upstream in upstreams().await {
                let Ok(Ok(mut server)) =
                    timeout(UPSTREAM_TIMEOUT, TcpStream::connect(upstream)).await
                else {
                    continue;
                };
                let _ = timeout(
                    TCP_SESSION_TIMEOUT,
                    copy_bidirectional(&mut client, &mut server),
                )
                .await;
                return;
            }
            debug!("No resolver accepted a TCP query from {peer}");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nameservers_are_read_in_order_and_junk_is_skipped() {
        let conf = "# generated\nsearch example.com\nnameserver 127.0.0.11\n\
                    nameserver  ::1  # local\nnameserver not-an-ip\noptions ndots:0\n";
        assert_eq!(
            nameservers(conf),
            [
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 11)),
                "::1".parse::<IpAddr>().unwrap()
            ]
        );
    }
}
