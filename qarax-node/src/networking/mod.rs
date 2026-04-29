pub mod bridge;
pub mod dhcp;
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

/// Validate an IPv4 CIDR string (e.g. "192.168.1.0/24").
/// Rejects malformed input before it reaches iptables arguments.
pub(super) fn validate_ipv4_cidr(cidr: &str) -> anyhow::Result<()> {
    let (ip_str, prefix_str) = cidr
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid CIDR {cidr:?}: missing '/'"))?;
    let _: std::net::Ipv4Addr = ip_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid CIDR {cidr:?}: bad IPv4 address"))?;
    let prefix: u8 = prefix_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid CIDR {cidr:?}: bad prefix length"))?;
    if prefix > 32 {
        anyhow::bail!("Invalid CIDR {cidr:?}: prefix length must be 0–32");
    }
    Ok(())
}

pub(super) fn validate_ipv4_address(ip: &str) -> anyhow::Result<()> {
    let _: std::net::Ipv4Addr = ip
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid IPv4 address {ip:?}"))?;
    Ok(())
}
