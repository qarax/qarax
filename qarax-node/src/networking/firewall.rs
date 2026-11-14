//! Per-NIC filtering of VM traffic in an nftables bridge table.
//!
//! Every frame a VM sends or receives passes its TAP's bridge port, whether
//! it goes to another VM on the same bridge, to the host, or is routed on, so
//! the rules hook there: `prerouting` matches frames from a TAP (`iifname`),
//! `postrouting` frames to one (`oifname`), each through a verdict map to that
//! NIC's chains.
//!
//! - `out_<nic>`: anti-spoofing. A VM may only send from its NIC's MAC and
//!   address (and ARP for them), plus the DHCP discovery that has no address
//!   yet. Traffic to the host itself (DHCP, DNS on the gateway) is accepted;
//!   the rest goes to `sgo_<nic>`.
//! - `in_<nic>`: ARP, DHCP replies and traffic from the host itself are
//!   accepted; the rest goes to `sgi_<nic>`. Routed traffic the host
//!   masqueraded to its own bridge address isn't the host's: conntrack tells
//!   them apart, and security-group rules match the source from before NAT.
//! - `sgo_<nic>` / `sgi_<nic>`: the VM's security groups, written only by
//!   [`sync_security_groups`] and empty (no filtering) for a VM without any.
//!   Being in the kernel, they survive a node restart that keeps its VMs.
//!
//! [`sync_nics`] writes the first two for every NIC on the node and removes the
//! chains of NICs that are gone; the security-group chains of NICs that stay
//! are left alone. Each call is one nft transaction, and calls are serialized.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::net::Ipv4Addr;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::debug;

use super::Ipv4Cidr;
use crate::rpc::node::{
    FirewallDirection, FirewallProtocol, NetConfig, VmFirewallInterface, VmFirewallRule,
};

const TABLE: &str = "bridge qarax";
/// Prefixes of the per-NIC chains; the rest of the name comes from the TAP.
const NIC_CHAINS: [&str; 4] = ["out_", "in_", "sgo_", "sgi_"];

static NFT: Mutex<()> = Mutex::const_new(());

/// A VM NIC on this node, as the firewall sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Nic {
    pub tap: String,
    pub mac: Option<[u8; 6]>,
    pub ip: Option<Ipv4Addr>,
    pub bridge: Option<String>,
}

impl Nic {
    /// The TAP-backed NICs in a VM's config (vhost-user NICs have no TAP).
    /// TAP names and MACs can come from users, and both end up in nft
    /// scripts, so anything unexpected is an error.
    pub fn from_config(networks: &[NetConfig]) -> Result<Vec<Nic>> {
        networks
            .iter()
            .filter_map(|net| Some((net, net.tap.as_deref()?)))
            .map(|(net, tap)| {
                if tap.is_empty()
                    || !tap
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
                {
                    bail!("Unsupported TAP name {tap:?}");
                }
                let mac = net
                    .mac
                    .as_deref()
                    .map(|mac| {
                        super::parse_mac(mac).with_context(|| format!("Invalid MAC {mac:?}"))
                    })
                    .transpose()?;
                Ok(Nic {
                    tap: tap.to_string(),
                    mac,
                    ip: net
                        .ip
                        .as_deref()
                        .and_then(|ip| ip.split('/').next()?.parse().ok()),
                    bridge: net.bridge.clone(),
                })
            })
            .collect()
    }

    /// The chain-name suffix for this NIC: its TAP name, as an nft identifier.
    fn suffix(&self) -> String {
        chain_suffix(&self.tap)
    }
}

fn chain_suffix(tap: &str) -> String {
    tap.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Make the per-NIC chains match `nics`, every TAP-backed NIC on this node.
/// `host_addrs` gives the host's own addresses on each bridge.
pub async fn sync_nics(nics: &[Nic], host_addrs: &HashMap<String, Vec<Ipv4Addr>>) -> Result<()> {
    let _guard = NFT.lock().await;
    let existing = existing_nic_suffixes().await?;
    let script = render_nics(nics, host_addrs, &existing);
    run_nft(&script).await
}

/// Apply a VM's security groups. `interfaces` are the NICs the control plane
/// filters (none when the VM has no security groups), by bridge and address,
/// each with its rules; the VM's other NICs are left unfiltered.
pub async fn sync_security_groups(
    vm_nics: &[Nic],
    interfaces: &[VmFirewallInterface],
) -> Result<()> {
    let mut filtered: HashMap<&str, Vec<Rule>> = HashMap::new();
    for iface in interfaces {
        let ip: Ipv4Addr = iface
            .ip
            .parse()
            .with_context(|| format!("Invalid interface address {:?}", iface.ip))?;
        let nic = vm_nics
            .iter()
            .find(|nic| nic.bridge.as_deref() == Some(&iface.bridge_name) && nic.ip == Some(ip))
            .with_context(|| {
                format!(
                    "The VM has no NIC with {ip} on bridge {}",
                    iface.bridge_name
                )
            })?;
        let rules = iface
            .rules
            .iter()
            .map(Rule::try_from)
            .collect::<Result<Vec<_>>>()?;
        filtered.insert(nic.tap.as_str(), rules);
    }

    let script = render_security_groups(vm_nics, &filtered);
    let _guard = NFT.lock().await;
    run_nft(&script).await
}

fn render_nics(
    nics: &[Nic],
    host_addrs: &HashMap<String, Vec<Ipv4Addr>>,
    existing: &BTreeSet<String>,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "add table {TABLE}");
    for map in ["nic_out", "nic_in"] {
        let _ = writeln!(s, "add map {TABLE} {map} {{ type ifname : verdict; }}");
        let _ = writeln!(s, "flush map {TABLE} {map}");
    }
    for (chain, hook, dir) in [
        ("prerouting", "prerouting", "iifname vmap @nic_out"),
        ("postrouting", "postrouting", "oifname vmap @nic_in"),
    ] {
        let _ = writeln!(
            s,
            "add chain {TABLE} {chain} {{ type filter hook {hook} priority filter; policy accept; }}"
        );
        let _ = writeln!(s, "flush chain {TABLE} {chain}");
        let _ = writeln!(s, "add rule {TABLE} {chain} {dir}");
    }

    let mut desired = BTreeSet::new();
    for nic in nics {
        let x = nic.suffix();
        let host = nic
            .bridge
            .as_ref()
            .and_then(|bridge| host_addrs.get(bridge))
            .filter(|addrs| !addrs.is_empty())
            .map(|addrs| {
                addrs
                    .iter()
                    .map(Ipv4Addr::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            });

        for chain in NIC_CHAINS {
            let _ = writeln!(s, "add chain {TABLE} {chain}{x}");
        }
        let _ = writeln!(s, "flush chain {TABLE} out_{x}");
        let _ = writeln!(s, "flush chain {TABLE} in_{x}");

        let out = |rule: &str| format!("add rule {TABLE} out_{x} {rule}\n");
        if let Some(mac) = nic.mac {
            let mac = mac.map(|b| format!("{b:02x}")).join(":");
            s += &out(&format!("ether saddr != {mac} drop"));
            s += &out(&format!("ether type arp arp saddr ether != {mac} drop"));
        }
        if let Some(ip) = nic.ip {
            s += &out(&format!(
                "ether type arp arp saddr ip != {{ {ip}, 0.0.0.0 }} drop"
            ));
        }
        s += &out("ether type arp accept");
        s += &out("ip saddr 0.0.0.0 ip daddr 255.255.255.255 udp dport 67 accept");
        if let Some(ip) = nic.ip {
            s += &out(&format!("ip saddr != {ip} drop"));
        }
        if let Some(host) = &host {
            s += &out(&format!("ip daddr {{ {host} }} accept"));
        }
        s += &out(&format!("jump sgo_{x}"));

        let _ = writeln!(s, "add rule {TABLE} in_{x} ether type arp accept");
        if let Some(host) = &host {
            // DHCP replies go to the broadcast address, so they aren't
            // replies to anything as far as conntrack knows.
            let _ = writeln!(
                s,
                "add rule {TABLE} in_{x} ip saddr {{ {host} }} udp sport 67 udp dport 68 accept"
            );
            let _ = writeln!(
                s,
                "add rule {TABLE} in_{x} ip saddr {{ {host} }} ct status & snat == 0 accept"
            );
        }
        let _ = writeln!(s, "add rule {TABLE} in_{x} jump sgi_{x}");

        let tap = &nic.tap;
        let _ = writeln!(
            s,
            "add element {TABLE} nic_out {{ \"{tap}\" : jump out_{x} }}"
        );
        let _ = writeln!(
            s,
            "add element {TABLE} nic_in {{ \"{tap}\" : jump in_{x} }}"
        );
        desired.insert(x);
    }

    // Chains of NICs that are gone: the verdict maps no longer point at them,
    // and out_/in_ (which jump to sg*_) go first.
    for x in existing.difference(&desired) {
        for chain in NIC_CHAINS {
            let _ = writeln!(s, "flush chain {TABLE} {chain}{x}");
        }
        for chain in NIC_CHAINS {
            let _ = writeln!(s, "delete chain {TABLE} {chain}{x}");
        }
    }
    s
}

fn render_security_groups(vm_nics: &[Nic], filtered: &HashMap<&str, Vec<Rule>>) -> String {
    let mut s = String::new();
    for nic in vm_nics {
        let x = nic.suffix();
        for chain in ["sgo_", "sgi_"] {
            let _ = writeln!(s, "add chain {TABLE} {chain}{x}");
            let _ = writeln!(s, "flush chain {TABLE} {chain}{x}");
        }
        let Some(rules) = filtered.get(nic.tap.as_str()) else {
            continue;
        };
        for chain in ["sgi_", "sgo_"] {
            let _ = writeln!(
                s,
                "add rule {TABLE} {chain}{x} ct state established,related accept"
            );
        }
        for rule in rules {
            let chain = match rule.direction {
                Direction::Ingress => "sgi_",
                Direction::Egress => "sgo_",
            };
            let _ = writeln!(s, "add rule {TABLE} {chain}{x} {}", rule.render());
        }
        // Ingress is denied unless allowed; egress only once it has rules.
        let _ = writeln!(s, "add rule {TABLE} sgi_{x} drop");
        if rules.iter().any(|r| r.direction == Direction::Egress) {
            let _ = writeln!(s, "add rule {TABLE} sgo_{x} drop");
        }
    }
    s
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Direction {
    Ingress,
    Egress,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Protocol {
    Any,
    Tcp,
    Udp,
    Icmp,
}

/// A security-group rule allowing traffic, validated.
#[derive(Clone, Debug, PartialEq)]
struct Rule {
    direction: Direction,
    protocol: Protocol,
    /// The peer: the source for ingress, the destination for egress.
    cidr: Option<Ipv4Cidr>,
    /// Destination ports, for TCP and UDP.
    ports: Option<(u16, u16)>,
}

impl TryFrom<&VmFirewallRule> for Rule {
    type Error = anyhow::Error;

    /// Anything not understood is an error, never a broader rule: a control
    /// plane newer than this node may send values it doesn't know.
    fn try_from(rule: &VmFirewallRule) -> Result<Self> {
        let direction = match FirewallDirection::try_from(rule.direction) {
            Ok(FirewallDirection::Ingress) => Direction::Ingress,
            Ok(FirewallDirection::Egress) => Direction::Egress,
            Err(_) => bail!("Unknown firewall direction {}", rule.direction),
        };
        let protocol = match FirewallProtocol::try_from(rule.protocol) {
            Ok(FirewallProtocol::Any) => Protocol::Any,
            Ok(FirewallProtocol::Tcp) => Protocol::Tcp,
            Ok(FirewallProtocol::Udp) => Protocol::Udp,
            Ok(FirewallProtocol::Icmp) => Protocol::Icmp,
            Err(_) => bail!("Unknown firewall protocol {}", rule.protocol),
        };
        let cidr = match rule.cidr.as_str() {
            "" => None,
            cidr => Some(cidr.parse()?),
        };
        let port = |p: i32| u16::try_from(p).with_context(|| format!("Invalid port {p}"));
        let ports = match (protocol, rule.port_start, rule.port_end) {
            (Protocol::Tcp | Protocol::Udp, Some(start), Some(end)) => {
                let (start, end) = (port(start)?, port(end)?);
                if start > end {
                    bail!("Invalid port range {start}-{end}");
                }
                Some((start, end))
            }
            _ => None,
        };
        Ok(Self {
            direction,
            protocol,
            cidr,
            ports,
        })
    }
}

impl Rule {
    /// The rule as nft match expressions ending in accept. Rules are IPv4.
    fn render(&self) -> String {
        let mut r = String::from("ether type ip");
        if let Some(cidr) = self.cidr {
            // Ingress: the source before NAT. Traffic the host routes from a
            // network it masquerades arrives from the bridge's own address.
            let field = match self.direction {
                Direction::Ingress => "ct original ip saddr",
                Direction::Egress => "ip daddr",
            };
            let _ = write!(r, " {field} {cidr}");
        }
        match (self.protocol, self.ports) {
            (Protocol::Any, _) => {}
            (Protocol::Icmp, _) => r += " ip protocol icmp",
            (Protocol::Tcp | Protocol::Udp, ports) => {
                let proto = if self.protocol == Protocol::Tcp {
                    "tcp"
                } else {
                    "udp"
                };
                let _ = write!(r, " ip protocol {proto}");
                match ports {
                    Some((start, end)) if start == end => {
                        let _ = write!(r, " {proto} dport {start}");
                    }
                    Some((start, end)) => {
                        let _ = write!(r, " {proto} dport {start}-{end}");
                    }
                    None => {}
                }
            }
        }
        r + " accept"
    }
}

/// Suffixes of the per-NIC chains in the table now.
async fn existing_nic_suffixes() -> Result<BTreeSet<String>> {
    let output = tokio::process::Command::new("nft")
        .args(["-j", "list", "chains", "bridge"])
        .output()
        .await
        .context("Failed to run nft")?;
    if !output.status.success() {
        bail!(
            "nft list chains failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let listing: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse nft's chain list")?;
    Ok(listing["nftables"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("chain"))
        .filter(|chain| chain["table"] == "qarax")
        .filter_map(|chain| chain["name"].as_str())
        .filter_map(|name| name.strip_prefix("out_"))
        .map(str::to_string)
        .collect())
}

pub(super) async fn run_nft(script: &str) -> Result<()> {
    debug!("nft script:\n{script}");
    let mut child = tokio::process::Command::new("nft")
        .args(["-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run nft")?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(script.as_bytes())
        .await
        .context("Failed to write the nft script")?;
    let output = child
        .wait_with_output()
        .await
        .context("Failed to wait for nft")?;
    if !output.status.success() {
        bail!(
            "nft failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(direction: FirewallDirection, protocol: i32, cidr: &str) -> VmFirewallRule {
        VmFirewallRule {
            direction: direction as i32,
            protocol,
            cidr: cidr.to_string(),
            port_start: Some(22),
            port_end: Some(22),
        }
    }

    #[test]
    fn rules_the_node_does_not_understand_are_rejected() {
        // A newer control plane's protocol must not become "any protocol".
        let unknown_protocol = rule(FirewallDirection::Ingress, 42, "");
        assert!(Rule::try_from(&unknown_protocol).is_err());
        let unknown_direction = VmFirewallRule {
            direction: 7,
            ..rule(FirewallDirection::Ingress, FirewallProtocol::Tcp as i32, "")
        };
        assert!(Rule::try_from(&unknown_direction).is_err());
        let bad_cidr = rule(
            FirewallDirection::Ingress,
            FirewallProtocol::Tcp as i32,
            "10.0.0.0/40",
        );
        assert!(Rule::try_from(&bad_cidr).is_err());
    }

    fn nic(tap: &str, ip: [u8; 4]) -> Nic {
        Nic {
            tap: tap.to_string(),
            mac: Some([0x02, 0, 0, 0, 0, 1]),
            ip: Some(Ipv4Addr::from(ip)),
            bridge: Some("br0".to_string()),
        }
    }

    #[test]
    fn an_ingress_rule_limits_its_source_protocol_and_port() {
        let r = Rule::try_from(&rule(
            FirewallDirection::Ingress,
            FirewallProtocol::Tcp as i32,
            "10.1.0.0/16",
        ))
        .unwrap();
        assert_eq!(
            r.render(),
            "ether type ip ct original ip saddr 10.1.0.0/16 ip protocol tcp tcp dport 22 accept"
        );
    }

    #[test]
    fn egress_is_denied_by_default_only_once_it_has_rules() {
        let a = nic("qta", [10, 0, 0, 2]);
        let ingress_only = HashMap::from([(
            "qta",
            vec![
                Rule::try_from(&rule(
                    FirewallDirection::Ingress,
                    FirewallProtocol::Icmp as i32,
                    "",
                ))
                .unwrap(),
            ],
        )]);
        let script = render_security_groups(std::slice::from_ref(&a), &ingress_only);
        assert!(script.contains("add rule bridge qarax sgi_qta drop"));
        assert!(!script.contains("sgo_qta drop"));

        // No security groups: both chains are flushed and left empty.
        let script = render_security_groups(&[a], &HashMap::new());
        assert!(!script.contains("add rule"));
    }

    #[test]
    fn chains_of_nics_that_are_gone_are_deleted() {
        let existing = BTreeSet::from(["qta".to_string(), "qtgone".to_string()]);
        let script = render_nics(&[nic("qta", [10, 0, 0, 2])], &HashMap::new(), &existing);
        for chain in NIC_CHAINS {
            assert!(script.contains(&format!("delete chain bridge qarax {chain}qtgone")));
            assert!(!script.contains(&format!("delete chain bridge qarax {chain}qta\n")));
        }
    }
}
