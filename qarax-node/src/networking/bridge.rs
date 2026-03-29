use anyhow::{Context, Result};
use futures::TryStreamExt;
use netlink_packet_route::{
    AddressFamily, address::AddressAttribute, link::LinkAttribute, route::RouteAddress,
    route::RouteAttribute,
};
use rtnetlink::{Handle, IpVersion};
use std::net::{IpAddr, Ipv4Addr};
use tracing::{debug, info};

async fn netlink_handle() -> Result<Handle> {
    let (conn, handle, _) =
        rtnetlink::new_connection().context("Failed to open netlink connection")?;
    tokio::spawn(conn);
    Ok(handle)
}

async fn link_index(handle: &Handle, name: &str) -> Result<u32> {
    Ok(link_message(handle, name).await?.header.index)
}

async fn link_message(
    handle: &Handle,
    name: &str,
) -> Result<netlink_packet_route::link::LinkMessage> {
    handle
        .link()
        .get()
        .match_name(name.to_string())
        .execute()
        .try_next()
        .await
        .with_context(|| format!("Failed to query link {name}"))?
        .ok_or_else(|| anyhow::anyhow!("Interface {name} not found"))
}

/// Extract MAC address bytes from a link message.
fn link_mac(link: &netlink_packet_route::link::LinkMessage) -> Option<Vec<u8>> {
    link.attributes.iter().find_map(|attr| {
        if let LinkAttribute::Address(mac) = attr {
            (mac.len() == 6).then(|| mac.clone())
        } else {
            None
        }
    })
}

/// Get all IPv4 addresses (ip, prefix_len) on an interface.
async fn ipv4_addrs(handle: &Handle, link_idx: u32) -> Result<Vec<(Ipv4Addr, u8)>> {
    let mut stream = handle
        .address()
        .get()
        .set_link_index_filter(link_idx)
        .execute();
    let mut result = Vec::new();
    while let Some(msg) = stream.try_next().await? {
        if msg.header.family != AddressFamily::Inet {
            continue;
        }
        let prefix_len = msg.header.prefix_len;
        for attr in &msg.attributes {
            if let AddressAttribute::Address(IpAddr::V4(ip)) = attr {
                result.push((*ip, prefix_len));
            }
        }
    }
    Ok(result)
}

/// Find the default IPv4 gateway going out via `via_idx`.
async fn default_gateway_via(handle: &Handle, via_idx: u32) -> Result<Option<Ipv4Addr>> {
    let mut stream = handle.route().get(IpVersion::V4).execute();
    while let Some(route) = stream.try_next().await? {
        if route.header.destination_prefix_length != 0 {
            continue;
        }
        let mut gw: Option<Ipv4Addr> = None;
        let mut oif: Option<u32> = None;
        for attr in &route.attributes {
            match attr {
                RouteAttribute::Gateway(RouteAddress::Inet(g)) => gw = Some(*g),
                RouteAttribute::Oif(i) => oif = Some(*i),
                _ => {}
            }
        }
        if oif == Some(via_idx) {
            return Ok(gw);
        }
    }
    Ok(None)
}

/// Delete a specific IPv4 address from an interface. Silently succeeds if not found.
async fn del_ipv4_addr(handle: &Handle, link_idx: u32, ip: Ipv4Addr, prefix: u8) -> Result<()> {
    let msgs: Vec<_> = {
        let mut stream = handle
            .address()
            .get()
            .set_link_index_filter(link_idx)
            .execute();
        let mut v = Vec::new();
        while let Some(msg) = stream.try_next().await? {
            if msg.header.family == AddressFamily::Inet
                && msg.header.prefix_len == prefix
                && msg
                    .attributes
                    .iter()
                    .any(|a| matches!(a, AddressAttribute::Address(IpAddr::V4(a)) if *a == ip))
            {
                v.push(msg);
            }
        }
        v
    };
    for msg in msgs {
        handle.address().del(msg).execute().await?;
    }
    Ok(())
}

/// Find physical NIC members of bridge `bridge_name` (excludes tap/veth).
async fn find_bridge_member(handle: &Handle, bridge_name: &str) -> Result<String> {
    let bridge_idx = link_index(handle, bridge_name)
        .await
        .with_context(|| format!("Failed to look up bridge {bridge_name}"))?;

    let mut stream = handle.link().get().execute();
    while let Some(link) = stream.try_next().await? {
        let mut master: Option<u32> = None;
        let mut name = String::new();
        for attr in &link.attributes {
            match attr {
                LinkAttribute::IfName(n) => name = n.clone(),
                LinkAttribute::Controller(idx) => master = Some(*idx),
                _ => {}
            }
        }
        if master == Some(bridge_idx)
            && !name.starts_with("tap")
            && !name.starts_with("veth")
            && !name.is_empty()
        {
            return Ok(name);
        }
    }
    anyhow::bail!("No physical member found for bridge {bridge_name}")
}

use super::validate_iface_name;

fn parse_cidr(cidr: &str) -> Result<(Ipv4Addr, u8)> {
    let (ip_str, prefix_str) = cidr
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid CIDR: {cidr}"))?;
    Ok((
        ip_str
            .parse()
            .with_context(|| format!("Invalid IP in CIDR: {cidr}"))?,
        prefix_str
            .parse()
            .with_context(|| format!("Invalid prefix in CIDR: {cidr}"))?,
    ))
}

/// Create a Linux bridge device and bring it up.
pub async fn create_bridge(name: &str) -> Result<()> {
    info!("Creating bridge: {name}");
    let handle = netlink_handle().await?;

    handle
        .link()
        .add()
        .bridge(name.to_string())
        .execute()
        .await
        .with_context(|| format!("Failed to create bridge {name}"))?;

    let idx = link_index(&handle, name).await?;
    handle
        .link()
        .set(idx)
        .up()
        .execute()
        .await
        .with_context(|| format!("Failed to bring up bridge {name}"))?;

    Ok(())
}

/// Assign an IP address to the bridge (gateway for the subnet).
pub async fn set_bridge_ip(name: &str, gateway_cidr: &str) -> Result<()> {
    info!("Setting bridge {name} IP to {gateway_cidr}");
    let (ip, prefix) = parse_cidr(gateway_cidr)?;
    let handle = netlink_handle().await?;
    let idx = link_index(&handle, name).await?;

    handle
        .address()
        .add(idx, ip.into(), prefix)
        .execute()
        .await
        .with_context(|| format!("Failed to set IP {gateway_cidr} on bridge {name}"))?;

    Ok(())
}

/// Delete a bridge device.
pub async fn delete_bridge(name: &str) -> Result<()> {
    info!("Deleting bridge: {name}");
    let handle = netlink_handle().await?;
    let idx = link_index(&handle, name).await?;

    handle
        .link()
        .del(idx)
        .execute()
        .await
        .with_context(|| format!("Failed to delete bridge {name}"))?;

    Ok(())
}

/// Attach a TAP device to a bridge.
pub async fn attach_to_bridge(tap_name: &str, bridge_name: &str) -> Result<()> {
    debug!("Attaching TAP {tap_name} to bridge {bridge_name}");
    let handle = netlink_handle().await?;
    let tap_idx = link_index(&handle, tap_name).await?;
    let bridge_idx = link_index(&handle, bridge_name).await?;

    handle
        .link()
        .set(tap_idx)
        .controller(bridge_idx)
        .execute()
        .await
        .with_context(|| format!("Failed to attach {tap_name} to bridge {bridge_name}"))?;

    Ok(())
}

/// Bridge an existing NIC: create a bridge, move the NIC's IP to the bridge,
/// and add the NIC as a member. VMs on this bridge share the NIC's L2 network.
///
/// The bridge is given the same MAC as the parent NIC so existing ARP entries
/// remain valid across the transition.  If systemd-networkd is running, runtime
/// configs are written so networkd stays in sync.
pub async fn bridge_interface(bridge_name: &str, parent_iface: &str) -> Result<()> {
    validate_iface_name(bridge_name)?;
    validate_iface_name(parent_iface)?;
    info!("Bridging interface {parent_iface} onto bridge {bridge_name}");
    let handle = netlink_handle().await?;

    // Gather state from parent NIC
    let parent_link = link_message(&handle, parent_iface).await?;
    let parent_idx = parent_link.header.index;

    let addrs = ipv4_addrs(&handle, parent_idx)
        .await
        .with_context(|| format!("Failed to get addresses for {parent_iface}"))?;
    let (parent_ip, parent_prefix) = addrs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No IPv4 address found on {parent_iface}"))?;

    let parent_mac =
        link_mac(&parent_link).ok_or_else(|| anyhow::anyhow!("No MAC found on {parent_iface}"))?;
    let parent_mac_display = parent_mac
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":");

    let default_gw = default_gateway_via(&handle, parent_idx).await?;
    let ip_cidr = format!("{parent_ip}/{parent_prefix}");

    // Write networkd overrides (if applicable) BEFORE migrating
    let use_networkd = is_networkd_running().await;
    if use_networkd {
        info!("systemd-networkd detected — writing bridge networkd configs");
        write_bridge_networkd_configs(
            bridge_name,
            parent_iface,
            &ip_cidr,
            default_gw.as_ref().map(|g| g.to_string()).as_deref(),
        )
        .await?;
    }

    // Create bridge with parent's MAC
    handle
        .link()
        .add()
        .bridge(bridge_name.to_string())
        .execute()
        .await
        .with_context(|| format!("Failed to create bridge {bridge_name}"))?;

    let bridge_idx = link_index(&handle, bridge_name).await?;

    handle
        .link()
        .set(bridge_idx)
        .address(parent_mac)
        .up()
        .execute()
        .await
        .with_context(|| format!("Failed to set MAC and bring up bridge {bridge_name}"))?;

    // Migrate IP: parent → bridge
    // Add to bridge first; enslaving then moves L3 processing there, so
    // there's zero window without an address on the bridge.
    handle
        .address()
        .add(bridge_idx, parent_ip.into(), parent_prefix)
        .execute()
        .await
        .with_context(|| format!("Failed to add IP to bridge {bridge_name}"))?;

    handle
        .link()
        .set(parent_idx)
        .controller(bridge_idx)
        .execute()
        .await
        .with_context(|| format!("Failed to add {parent_iface} to bridge {bridge_name}"))?;

    // Remove (now-shadowed) IP from parent — may already be gone after enslave.
    del_ipv4_addr(&handle, parent_idx, parent_ip, parent_prefix)
        .await
        .ok();

    if let Some(gw) = default_gw {
        // Use replace semantics (NLM_F_REPLACE|NLM_F_CREATE) so this works
        // whether or not the old route was auto-removed when parent was enslaved.
        handle
            .route()
            .add()
            .v4()
            .gateway(gw)
            .output_interface(bridge_idx)
            .replace()
            .execute()
            .await
            .context("Failed to restore default route")?;
    }

    if use_networkd {
        let br = bridge_name.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let status = tokio::process::Command::new("networkctl")
                .arg("reload")
                .status()
                .await;
            if let Err(e) = status {
                tracing::error!("Failed to reload networkd for bridge {br}: {e}");
            }
        });
    }

    info!(
        "Bridge {bridge_name} created with parent {parent_iface} \
         (IP: {ip_cidr}, MAC: {parent_mac_display})"
    );
    Ok(())
}

/// Undo bridge_interface: move IP back from bridge to parent NIC, remove
/// parent from bridge, and delete the bridge.
pub async fn unbridge_interface(bridge_name: &str) -> Result<()> {
    validate_iface_name(bridge_name)?;
    info!("Unbridging {bridge_name}");
    let handle = netlink_handle().await?;

    let parent_iface = find_bridge_member(&handle, bridge_name).await?;

    // Clean up networkd runtime configs if present
    let dir = "/run/systemd/network";
    let _ = tokio::fs::remove_file(format!("{dir}/05-{bridge_name}.netdev")).await;
    let _ = tokio::fs::remove_file(format!("{dir}/05-{parent_iface}-bridge-member.network")).await;
    let _ = tokio::fs::remove_file(format!("{dir}/05-{bridge_name}.network")).await;

    let bridge_idx = link_index(&handle, bridge_name).await?;

    if is_networkd_running().await {
        // Delete bridge and let networkd reconfigure the parent NIC from its
        // persistent config in /etc/systemd/network/.
        handle.link().del(bridge_idx).execute().await.ok();
        tokio::process::Command::new("networkctl")
            .arg("reload")
            .status()
            .await
            .ok();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    } else {
        let parent_idx = link_index(&handle, &parent_iface).await?;

        let ip_cidr = ipv4_addrs(&handle, bridge_idx)
            .await
            .ok()
            .and_then(|v| v.into_iter().next());

        let default_gw = default_gateway_via(&handle, bridge_idx)
            .await
            .ok()
            .flatten();

        // Remove parent from bridge (IFLA_MASTER = 0 means no master)
        handle
            .link()
            .set(parent_idx)
            .controller(0)
            .execute()
            .await
            .ok();

        // Restore IP on parent
        if let Some((ip, prefix)) = ip_cidr {
            handle
                .address()
                .add(parent_idx, ip.into(), prefix)
                .execute()
                .await
                .ok();
        }

        // Delete bridge
        handle.link().del(bridge_idx).execute().await.ok();

        // Restore default route via parent
        if let Some(gw) = default_gw {
            handle
                .route()
                .add()
                .v4()
                .gateway(gw)
                .output_interface(parent_idx)
                .execute()
                .await
                .ok();
        }
    }

    info!("Unbridged {bridge_name} (parent: {parent_iface})");
    Ok(())
}

/// Check if a bridge has a physical NIC member (i.e., was created by bridge_interface).
pub async fn is_bridged_interface(bridge_name: &str) -> bool {
    let Ok(handle) = netlink_handle().await else {
        return false;
    };
    find_bridge_member(&handle, bridge_name).await.is_ok()
}

async fn is_networkd_running() -> bool {
    tokio::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "systemd-networkd"])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn write_bridge_networkd_configs(
    bridge_name: &str,
    parent_iface: &str,
    ip_cidr: &str,
    gateway: Option<&str>,
) -> Result<()> {
    let dir = "/run/systemd/network";
    tokio::fs::create_dir_all(dir).await.ok();

    let netdev = format!("[NetDev]\nName={bridge_name}\nKind=bridge\n");
    tokio::fs::write(format!("{dir}/05-{bridge_name}.netdev"), &netdev).await?;

    let parent_net = format!("[Match]\nName={parent_iface}\n\n[Network]\nBridge={bridge_name}\n");
    tokio::fs::write(
        format!("{dir}/05-{parent_iface}-bridge-member.network"),
        &parent_net,
    )
    .await?;

    let mut bridge_net =
        format!("[Match]\nName={bridge_name}\n\n[Network]\nAddress={ip_cidr}\nDNS=8.8.8.8\n");
    if let Some(gw) = gateway {
        bridge_net.push_str(&format!("Gateway={gw}\n"));
    }
    tokio::fs::write(format!("{dir}/05-{bridge_name}.network"), &bridge_net).await?;

    Ok(())
}
