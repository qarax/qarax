//! An in-process DHCP server per bridge that answers only this node's VM NICs.
//!
//! The control plane allocates every NIC's address (IPAM) and the node knows
//! each NIC's MAC, so the server hands out exactly those reservations and keeps
//! no lease state of its own: a node restart or a re-created VM can't drift
//! from IPAM, and on a bridge carrying a physical NIC, the other machines on
//! that LAN never hear from it.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::{Context, Result};
use edge_dhcp::server::{Action, ServerOptions};
use edge_dhcp::{DhcpOption, Options, Packet};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use super::Ipv4Cidr;

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;
const LEASE_DURATION_SECS: u32 = 43200; // 12 hours

static SERVERS: LazyLock<Mutex<HashMap<String, JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Where a bridge's DHCP server looks up the address reserved for a MAC.
#[async_trait::async_trait]
pub trait Reservations: Send + Sync {
    /// The address of this node's VM NIC with `mac` on `bridge`, if there is one.
    async fn reserved_ip(&self, bridge: &str, mac: [u8; 6]) -> Option<Ipv4Addr>;
}

/// What a bridge's DHCP server tells its clients.
pub struct DhcpNetwork {
    pub subnet: Ipv4Cidr,
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
}

/// Start (or restart) the DHCP server on a bridge.
pub async fn start_dhcp_server(
    bridge: &str,
    network: DhcpNetwork,
    reservations: Arc<dyn Reservations>,
) -> Result<()> {
    // The server identifier is an address of this host on the bridge, which
    // clients unicast renewals to: the gateway on an isolated network, the
    // host's own LAN address on a bridged one (where the gateway is a router).
    let server_ip = super::bridge::first_ipv4(bridge)
        .await
        .context("Failed to read the bridge's address for the DHCP server identifier")?;
    info!(
        "Starting DHCP server on {bridge} (server {server_ip}, subnet {}, gw={}, dns={})",
        network.subnet, network.gateway, network.dns
    );

    // Hold the lock across stop + bind + insert so concurrent starts for one
    // bridge don't race.
    let mut servers = SERVERS.lock().unwrap();
    if let Some(handle) = servers.remove(bridge) {
        info!("Stopping existing DHCP server for bridge {bridge}");
        handle.abort();
    }

    let socket = create_dhcp_socket(bridge)?;
    let bridge_name = bridge.to_string();
    let handle = tokio::spawn(async move {
        if let Err(e) = run_dhcp_loop(socket, server_ip, network, reservations, &bridge_name).await
        {
            tracing::error!("DHCP server for {bridge_name} exited with error: {e}");
        }
    });
    servers.insert(bridge.to_string(), handle);
    Ok(())
}

/// Stop the DHCP server for a bridge.
pub async fn stop_dhcp_server(bridge: &str) -> Result<()> {
    if let Some(handle) = SERVERS.lock().unwrap().remove(bridge) {
        info!("Stopping DHCP server for bridge {bridge}");
        handle.abort();
    }
    Ok(())
}

fn create_dhcp_socket(bridge: &str) -> Result<UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .context("Failed to create UDP socket")?;

    socket
        .set_reuse_address(true)
        .context("Failed to set SO_REUSEADDR")?;
    socket
        .set_broadcast(true)
        .context("Failed to set SO_BROADCAST")?;
    socket
        .bind_device(Some(bridge.as_bytes()))
        .context("Failed to bind socket to bridge interface")?;
    socket
        .bind(&socket2::SockAddr::from(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DHCP_SERVER_PORT,
        )))
        .context("Failed to bind to DHCP server port")?;
    socket
        .set_nonblocking(true)
        .context("Failed to set socket non-blocking")?;

    UdpSocket::from_std(socket.into()).context("Failed to create tokio UdpSocket")
}

fn server_options<'a>(
    server_ip: Ipv4Addr,
    network: &DhcpNetwork,
    gateways: &'a [Ipv4Addr],
    dns: &'a [Ipv4Addr],
) -> ServerOptions<'a> {
    let mut options = ServerOptions::new(server_ip, None);
    options.gateways = gateways;
    options.dns = dns;
    options.subnet = Some(network.subnet.netmask());
    options.lease_duration_secs = LEASE_DURATION_SECS;
    options
}

/// The reply to a client request, given the address reserved for its MAC.
/// Clients without a reservation get none: they aren't this node's VMs.
fn reply<'o>(
    options: &ServerOptions<'o>,
    request: &Packet<'_>,
    action: &Action<'_>,
    reserved: Option<Ipv4Addr>,
    opt_buf: &'o mut [DhcpOption<'o>],
) -> Option<Packet<'o>> {
    let ip = reserved?;
    match action {
        Action::Discover(..) => Some(options.offer(request, ip, opt_buf)),
        // A client asking for anything but its reservation (an address from
        // an earlier allocation, say) is told no, and starts over.
        Action::Request(requested, _) => {
            Some(options.ack_nak(request, (*requested == ip).then_some(ip), opt_buf))
        }
        Action::Release(..) | Action::Decline(..) => None,
    }
}

fn client_mac(action: &Action<'_>) -> [u8; 6] {
    let chaddr = match action {
        Action::Discover(_, chaddr)
        | Action::Request(_, chaddr)
        | Action::Release(_, chaddr)
        | Action::Decline(_, chaddr) => chaddr,
    };
    chaddr[..6].try_into().expect("chaddr holds 16 bytes")
}

async fn run_dhcp_loop(
    socket: UdpSocket,
    server_ip: Ipv4Addr,
    network: DhcpNetwork,
    reservations: Arc<dyn Reservations>,
    bridge: &str,
) -> Result<()> {
    let gateways = [network.gateway];
    let dns = [network.dns];
    let options = server_options(server_ip, &network, &gateways, &dns);
    let mut buf = [0u8; 1500];

    loop {
        let (len, _src) = socket.recv_from(&mut buf).await?;

        let request = match Packet::decode(&buf[..len]) {
            Ok(pkt) => pkt,
            Err(e) => {
                debug!("Invalid DHCP packet on {bridge}: {e:?}");
                continue;
            }
        };
        let Some(action) = options.process(&request) else {
            continue;
        };
        let reserved = reservations.reserved_ip(bridge, client_mac(&action)).await;

        let mut opt_buf = Options::buf();
        let Some(reply) = reply(&options, &request, &action, reserved, &mut opt_buf) else {
            continue;
        };

        // RFC 2131 Section 4.1 destination rules
        let (dest_ip, dest_port) = if !request.giaddr.is_unspecified() {
            (request.giaddr, DHCP_SERVER_PORT)
        } else if !request.ciaddr.is_unspecified() && !request.broadcast {
            (request.ciaddr, DHCP_CLIENT_PORT)
        } else {
            (Ipv4Addr::BROADCAST, DHCP_CLIENT_PORT)
        };

        let mut out = [0u8; 1500];
        match reply.encode(&mut out) {
            Ok(encoded) => {
                let dest = SocketAddrV4::new(dest_ip, dest_port);
                if let Err(e) = socket.send_to(encoded, dest).await {
                    warn!("Failed to send DHCP reply on {bridge}: {e}");
                }
            }
            Err(e) => warn!("Failed to encode DHCP reply on {bridge}: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use edge_dhcp::MessageType;

    use super::*;

    const MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const RESERVED: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 5);
    // Ask for the router, subnet mask and DNS server, as real clients do.
    const PARAMS: &[u8] = &[
        DhcpOption::CODE_ROUTER,
        DhcpOption::CODE_SUBNET,
        DhcpOption::CODE_DNS,
    ];

    fn network() -> DhcpNetwork {
        DhcpNetwork {
            subnet: "10.0.0.0/23".parse().unwrap(),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            dns: Ipv4Addr::new(10, 0, 0, 1),
        }
    }

    fn message_type(packet: &Packet<'_>) -> Option<MessageType> {
        packet.options.iter().find_map(|o| match o {
            DhcpOption::MessageType(t) => Some(t),
            _ => None,
        })
    }

    /// The reply to a request carrying `options`, from MAC, when the node
    /// reserves `reserved` for it.
    fn answer(
        request_options: &[DhcpOption<'_>],
        reserved: Option<Ipv4Addr>,
    ) -> Option<(MessageType, Ipv4Addr, Option<Ipv4Addr>)> {
        let network = network();
        let (gateways, dns) = ([network.gateway], [network.dns]);
        let options = server_options(network.gateway, &network, &gateways, &dns);
        let request = Packet::new_request(MAC, 7, 0, None, true, Options::new(request_options));
        let action = options.process(&request)?;
        let mut opt_buf = Options::buf();
        let reply = reply(&options, &request, &action, reserved, &mut opt_buf)?;
        let mask = reply.options.iter().find_map(|o| match o {
            DhcpOption::SubnetMask(m) => Some(m),
            _ => None,
        });
        Some((message_type(&reply)?, reply.yiaddr, mask))
    }

    #[test]
    fn a_reserved_nic_is_offered_its_address_with_the_networks_mask() {
        let discover = [
            DhcpOption::MessageType(MessageType::Discover),
            DhcpOption::ParameterRequestList(PARAMS),
        ];
        assert_eq!(
            answer(&discover, Some(RESERVED)),
            Some((
                MessageType::Offer,
                RESERVED,
                Some(Ipv4Addr::new(255, 255, 254, 0))
            ))
        );
    }

    #[test]
    fn a_request_is_acked_only_for_the_reserved_address() {
        let request = |ip| {
            [
                DhcpOption::MessageType(MessageType::Request),
                DhcpOption::RequestedIpAddress(ip),
                DhcpOption::ParameterRequestList(PARAMS),
            ]
        };
        let ack = answer(&request(RESERVED), Some(RESERVED));
        assert_eq!(
            ack.map(|(t, ip, _)| (t, ip)),
            Some((MessageType::Ack, RESERVED))
        );

        let stale = answer(&request(Ipv4Addr::new(10, 0, 0, 9)), Some(RESERVED));
        assert_eq!(stale.map(|(t, ..)| t), Some(MessageType::Nak));
    }

    #[test]
    fn clients_without_a_reservation_get_no_answer() {
        // e.g. other machines on a LAN the bridge is attached to.
        let discover = [DhcpOption::MessageType(MessageType::Discover)];
        assert_eq!(answer(&discover, None), None);
        let request = [
            DhcpOption::MessageType(MessageType::Request),
            DhcpOption::RequestedIpAddress(RESERVED),
        ];
        assert_eq!(answer(&request, None), None);
    }
}
