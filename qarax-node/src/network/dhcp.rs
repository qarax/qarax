use crate::network::MacAddress;
use smoltcp::dhcp::{ClientState, Dhcpv4Client};
use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::wait as phy_wait;
use smoltcp::phy::TapInterface;
use smoltcp::socket::{RawPacketMetadata, RawSocketBuffer, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpCidr, Ipv4Address};
use std::collections::BTreeMap;
use std::error::Error;
use std::os::unix::io::AsRawFd;

// Get an IP address from the DHCP server, based on https://github.com/smoltcp-rs/smoltcp/blob/master/examples/dhcp_client.rs
pub fn get_ip(
    mac_address: MacAddress,
    tap_device: &str,
) -> Result<String, Box<dyn Error + Sync + Send>> {
    tracing::info!("Starting DHCP client...");

    // TODO: error handling: ensure tap device exists and stuff
    let mut sockets = SocketSet::new(vec![]);
    let tx_buffer = RawSocketBuffer::new([RawPacketMetadata::EMPTY; 1], vec![0; 600]);
    let rx_buffer = RawSocketBuffer::new([RawPacketMetadata::EMPTY; 1], vec![0; 600]);
    let tap = TapInterface::new(tap_device).unwrap();
    let mut routes_storage = [None; 1];
    let routes = Routes::new(&mut routes_storage[..]);
    let fd = tap.as_raw_fd();

    let mut iface = EthernetInterfaceBuilder::new(tap)
        .neighbor_cache(NeighborCache::new(BTreeMap::new()))
        .any_ip(true)
        .ip_addrs([IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)])
        .ethernet_addr(EthernetAddress(mac_address.0))
        .routes(routes)
        .finalize();

    let mut dhcp = Dhcpv4Client::new(&mut sockets, rx_buffer, tx_buffer, Instant::now());
    let mut ip_address = String::new();
    loop {
        match dhcp.state {
            ClientState::Renew(_) => {
                tracing::info!(
                    "Got an ACK from DHCP server, return IP address '{}'",
                    ip_address
                );
                break Ok(ip_address);
            }
            _ => {}
        }

        let timestamp = Instant::now();
        iface
            .poll(&mut sockets, timestamp)
            .map(|_| ())
            .unwrap_or_else(|e| tracing::debug!("Poll: {:?}", e));
        let config = dhcp
            .poll(&mut iface, &mut sockets, timestamp)
            .unwrap_or_else(|e| {
                tracing::info!("err: {}", e);
                None
            });

        match config {
            Some(c) => {
                if c.address.is_some() {
                    iface.update_ip_addrs(|addrs| {
                        addrs.iter_mut().nth(0).map(|addr| {
                            *addr = IpCidr::Ipv4(c.address.unwrap());
                        });
                    });
                    ip_address = c.address.unwrap().address().to_string();
                    tracing::info!("Assigned an IPv4 address: {}", c.address.unwrap());
                }
            }

            None => {}
        };

        let mut timeout = dhcp.next_poll(timestamp);
        dhcp.next_poll(Instant::now());
        iface
            .poll_delay(&sockets, timestamp)
            .map(|sockets_timeout| timeout = sockets_timeout);
        phy_wait(fd, Some(timeout)).unwrap_or_else(|e| tracing::info!("Wait: {:?}", e));
    }
}
