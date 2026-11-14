use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use sha2::{Digest, Sha256};

pub mod bridge;
pub mod dhcp;
pub mod dns;
pub mod firewall;
pub mod iptables;
pub mod vxlan;

/// Validate a network interface name.
///
/// Enforces kernel rules (IFNAMSIZ=16, no '/' or NUL) plus additional
/// restrictions needed when names appear in file paths (path traversal),
/// systemd.network files (whitespace breaks ini parsing), or are passed to
/// command-line tools like iptables (leading '-' is misread as a flag).
pub(super) fn validate_iface_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.len() > 15 {
        anyhow::bail!("Interface name {name:?} has invalid length (must be 1–15 chars)");
    }
    if name.starts_with('-') {
        anyhow::bail!("Interface name {name:?} must not start with '-'");
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\0' || c.is_whitespace())
    {
        anyhow::bail!("Interface name {name:?} contains illegal characters");
    }
    Ok(())
}

/// An IPv4 address with a prefix length, e.g. "10.0.0.1/24".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Cidr {
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

impl Ipv4Cidr {
    pub fn netmask(&self) -> Ipv4Addr {
        Ipv4Addr::from(
            u32::MAX
                .checked_shl(32 - u32::from(self.prefix))
                .unwrap_or(0),
        )
    }
}

impl FromStr for Ipv4Cidr {
    type Err = anyhow::Error;

    fn from_str(cidr: &str) -> anyhow::Result<Self> {
        let (ip_str, prefix_str) = cidr
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("Invalid CIDR {cidr:?}: missing '/'"))?;
        let addr = ip_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid CIDR {cidr:?}: bad IPv4 address"))?;
        let prefix = prefix_str
            .parse()
            .ok()
            .filter(|prefix| *prefix <= 32)
            .ok_or_else(|| anyhow::anyhow!("Invalid CIDR {cidr:?}: prefix must be 0-32"))?;
        Ok(Self { addr, prefix })
    }
}

impl fmt::Display for Ipv4Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// Validate an IPv4 CIDR string (e.g. "192.168.1.0/24").
/// Rejects malformed input before it reaches iptables arguments.
pub(super) fn validate_ipv4_cidr(cidr: &str) -> anyhow::Result<()> {
    cidr.parse::<Ipv4Cidr>().map(|_| ())
}

pub(super) fn validate_ipv4_address(ip: &str) -> anyhow::Result<()> {
    let _: std::net::Ipv4Addr = ip
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid IPv4 address {ip:?}"))?;
    Ok(())
}

/// The MAC address of a VM NIC that has none configured: locally administered,
/// unicast, and derived from the VM and device IDs. A NIC keeps it across
/// restarts and migrations without the node storing anything, and the node
/// knows every NIC's MAC (for DHCP reservations and the firewall).
pub fn nic_mac(vm_id: &str, device_id: &str) -> String {
    let digest = Sha256::digest(format!("{vm_id}/{device_id}"));
    let mut mac = [0x02; 6];
    mac[1..].copy_from_slice(&digest[..5]);
    mac.map(|b| format!("{b:02x}")).join(":")
}

pub fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let mut bytes = [0u8; 6];
    let mut parts = mac.split(':');
    for byte in &mut bytes {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_parses_and_gives_its_netmask() {
        let cidr: Ipv4Cidr = "10.1.2.3/23".parse().unwrap();
        assert_eq!(cidr.addr, Ipv4Addr::new(10, 1, 2, 3));
        assert_eq!(cidr.netmask(), Ipv4Addr::new(255, 255, 254, 0));
        assert_eq!(
            "0.0.0.0/0".parse::<Ipv4Cidr>().unwrap().netmask(),
            Ipv4Addr::UNSPECIFIED
        );
        for bad in ["10.0.0.0", "10.0.0.0/33", "10.0.0/24", "10.0.0.0/x"] {
            assert!(bad.parse::<Ipv4Cidr>().is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn nic_macs_are_stable_unicast_and_distinct_per_nic() {
        let vm = "6f1c2a9e-0000-4000-8000-000000000001";
        let mac = nic_mac(vm, "net0");
        assert_eq!(mac, nic_mac(vm, "net0"));
        assert_ne!(mac, nic_mac(vm, "net1"));
        let bytes = parse_mac(&mac).unwrap();
        // Locally administered, not multicast.
        assert_eq!(bytes[0] & 0b11, 0b10);
    }
}
