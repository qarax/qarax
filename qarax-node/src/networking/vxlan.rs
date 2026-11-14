use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::{Context, Result};

use crate::rpc::node::VpcOverlayConfig;

const QARAX_VXLAN_PREFIX: &str = "qvx";
const VXLAN_FDB_MAC: &str = "00:00:00:00:00:00";
const VXLAN_PORT: u16 = 4789;
/// What VXLAN over IPv4 adds to a packet: the outer IP (20) and UDP (8)
/// headers, the VXLAN header (8) and the inner Ethernet header (14).
const VXLAN_OVERHEAD: u32 = 50;

pub async fn sync_vpc_overlays(overlays: &[VpcOverlayConfig]) -> Result<()> {
    let desired_ifaces: HashSet<&str> = overlays
        .iter()
        .map(|overlay| overlay.interface_name.as_str())
        .collect();

    for overlay in overlays {
        reconcile_overlay(overlay).await?;
    }

    for iface in list_managed_interfaces().await? {
        if !desired_ifaces.contains(iface.as_str()) {
            delete_interface(&iface).await?;
        }
    }

    sync_underlay_filter(overlays).await
}

/// Accept VXLAN traffic only from overlay peers. The overlays carry VPC
/// traffic unauthenticated, so a packet from anyone else on the underlay with
/// a VPC's VNI would land in it. Without overlays there's nothing to filter,
/// and the table goes (VXLAN on this port may then belong to someone else).
async fn sync_underlay_filter(overlays: &[VpcOverlayConfig]) -> Result<()> {
    let peers: BTreeSet<&str> = overlays
        .iter()
        .flat_map(|overlay| overlay.peer_underlay_ips.iter().map(String::as_str))
        .collect();
    let table = "inet qarax_vxlan";
    let mut script = format!("add table {table}\n");
    if overlays.is_empty() {
        script += &format!("delete table {table}\n");
    } else {
        script += &format!(
            "add set {table} peers {{ type ipv4_addr; }}\n\
             flush set {table} peers\n\
             add chain {table} input {{ type filter hook input priority filter; policy accept; }}\n\
             flush chain {table} input\n\
             add rule {table} input udp dport {VXLAN_PORT} ip saddr != @peers drop\n"
        );
        if !peers.is_empty() {
            let peers = peers.into_iter().collect::<Vec<_>>().join(", ");
            script += &format!("add element {table} peers {{ {peers} }}\n");
        }
    }
    super::firewall::run_nft(&script)
        .await
        .context("Failed to sync the VXLAN underlay filter")
}

async fn reconcile_overlay(overlay: &VpcOverlayConfig) -> Result<()> {
    super::validate_iface_name(&overlay.interface_name)?;
    super::validate_ipv4_address(&overlay.local_underlay_ip)?;
    super::validate_ipv4_cidr(&overlay.local_overlay_cidr)?;

    for peer_ip in &overlay.peer_underlay_ips {
        super::validate_ipv4_address(peer_ip)?;
    }
    for route in &overlay.routes {
        super::validate_ipv4_cidr(&route.subnet)?;
        super::validate_ipv4_address(&route.via_ip)?;
    }

    if overlay_needs_recreate(overlay).await? {
        delete_interface(&overlay.interface_name).await?;
        create_interface(overlay).await?;
    }

    sync_interface_address(&overlay.interface_name, &overlay.local_overlay_cidr).await?;

    // Sized so an encapsulated packet fits the underlay unfragmented; the
    // overlay is routed, so guests sending larger packets get told to shrink
    // them (or, without DF, have them fragmented here, before encapsulation).
    let underlay_ip = overlay.local_underlay_ip.parse()?;
    let mtu = super::bridge::mtu_of_interface_with(underlay_ip)
        .await
        .context("Failed to read the underlay MTU")?
        - VXLAN_OVERHEAD;
    run_cmd(
        "ip",
        &[
            "link",
            "set",
            "dev",
            &overlay.interface_name,
            "mtu",
            &mtu.to_string(),
            "up",
        ],
    )
    .await
    .with_context(|| format!("Failed to bring up {}", overlay.interface_name))?;

    sync_fdb_entries(&overlay.interface_name, &overlay.peer_underlay_ips).await?;
    sync_routes(&overlay.interface_name, &overlay.routes).await?;

    Ok(())
}

async fn list_managed_interfaces() -> Result<Vec<String>> {
    let output = run_cmd_capture("ip", &["-o", "link", "show"]).await?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(':')?;
            let iface = rest.trim().split(':').next()?.trim();
            iface
                .starts_with(QARAX_VXLAN_PREFIX)
                .then(|| iface.to_string())
        })
        .collect())
}

async fn delete_interface(name: &str) -> Result<()> {
    super::validate_iface_name(name)?;
    let result = run_cmd("ip", &["link", "del", "dev", name]).await;
    match result {
        Ok(()) => Ok(()),
        Err(error)
            if error.to_string().contains("Cannot find device")
                || error.to_string().contains("does not exist") =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn overlay_needs_recreate(overlay: &VpcOverlayConfig) -> Result<bool> {
    let output = match run_cmd_capture(
        "ip",
        &["-d", "link", "show", "dev", &overlay.interface_name],
    )
    .await
    {
        Ok(output) => output,
        Err(error)
            if error.to_string().contains("Cannot find device")
                || error.to_string().contains("does not exist") =>
        {
            return Ok(true);
        }
        Err(error) => return Err(error),
    };

    let expected_vni = format!("vxlan id {}", overlay.vni);
    let expected_local = format!("local {}", overlay.local_underlay_ip);
    Ok(!(output.contains(&expected_vni)
        && output.contains(&expected_local)
        && output.contains(&format!("dstport {VXLAN_PORT}"))
        && output.contains("nolearning")))
}

async fn create_interface(overlay: &VpcOverlayConfig) -> Result<()> {
    run_cmd(
        "ip",
        &[
            "link",
            "add",
            "dev",
            &overlay.interface_name,
            "type",
            "vxlan",
            "id",
            &overlay.vni.to_string(),
            "dstport",
            &VXLAN_PORT.to_string(),
            "local",
            &overlay.local_underlay_ip,
            "nolearning",
        ],
    )
    .await
    .with_context(|| {
        format!(
            "Failed to create VXLAN interface {}",
            overlay.interface_name
        )
    })
}

async fn sync_interface_address(interface_name: &str, local_overlay_cidr: &str) -> Result<()> {
    for address in list_interface_addresses(interface_name).await? {
        if address != local_overlay_cidr {
            run_cmd("ip", &["address", "del", &address, "dev", interface_name])
                .await
                .with_context(|| {
                    format!(
                        "Failed to remove stale overlay IP {} from {}",
                        address, interface_name
                    )
                })?;
        }
    }

    run_cmd(
        "ip",
        &[
            "address",
            "replace",
            local_overlay_cidr,
            "dev",
            interface_name,
        ],
    )
    .await
    .with_context(|| format!("Failed to assign overlay IP to {}", interface_name))
}

async fn list_interface_addresses(interface_name: &str) -> Result<Vec<String>> {
    let output =
        run_cmd_capture("ip", &["-o", "-4", "addr", "show", "dev", interface_name]).await?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            while let Some(part) = parts.next() {
                if part == "inet" {
                    return parts.next().map(ToString::to_string);
                }
            }
            None
        })
        .collect())
}

async fn sync_fdb_entries(interface_name: &str, peer_underlay_ips: &[String]) -> Result<()> {
    let desired_peers: BTreeSet<String> = peer_underlay_ips.iter().cloned().collect();
    let existing_peers = list_fdb_peer_ips(interface_name).await?;

    for peer_ip in existing_peers.difference(&desired_peers) {
        run_cmd(
            "bridge",
            &[
                "fdb",
                "del",
                VXLAN_FDB_MAC,
                "dev",
                interface_name,
                "dst",
                peer_ip.as_str(),
                "self",
                "permanent",
            ],
        )
        .await
        .with_context(|| {
            format!(
                "Failed to remove flood entry for peer {} on {}",
                peer_ip, interface_name
            )
        })?;
    }

    for peer_ip in desired_peers.difference(&existing_peers) {
        run_cmd(
            "bridge",
            &[
                "fdb",
                "append",
                VXLAN_FDB_MAC,
                "dev",
                interface_name,
                "dst",
                peer_ip.as_str(),
                "self",
                "permanent",
            ],
        )
        .await
        .with_context(|| {
            format!(
                "Failed to sync flood entry for peer {} on {}",
                peer_ip, interface_name
            )
        })?;
    }

    Ok(())
}

async fn list_fdb_peer_ips(interface_name: &str) -> Result<BTreeSet<String>> {
    let output = run_cmd_capture("bridge", &["fdb", "show", "dev", interface_name]).await?;
    Ok(output
        .lines()
        .filter(|line| line.contains(VXLAN_FDB_MAC))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            while let Some(part) = parts.next() {
                if part == "dst" {
                    return parts.next().map(ToString::to_string);
                }
            }
            None
        })
        .collect())
}

async fn sync_routes(
    interface_name: &str,
    routes: &[crate::rpc::node::VpcOverlayRoute],
) -> Result<()> {
    let desired_routes: BTreeMap<&str, &str> = routes
        .iter()
        .map(|route| (route.subnet.as_str(), route.via_ip.as_str()))
        .collect();
    let existing_routes = list_routes(interface_name).await?;

    for subnet in existing_routes.keys() {
        if desired_routes.contains_key(subnet.as_str()) {
            continue;
        }
        run_cmd(
            "ip",
            &[
                "route",
                "del",
                subnet,
                "dev",
                interface_name,
                "proto",
                "static",
            ],
        )
        .await
        .with_context(|| {
            format!(
                "Failed to remove stale route {} from {}",
                subnet, interface_name
            )
        })?;
    }

    for route in routes {
        run_cmd(
            "ip",
            &[
                "route",
                "replace",
                &route.subnet,
                "via",
                &route.via_ip,
                "dev",
                interface_name,
                "proto",
                "static",
            ],
        )
        .await
        .with_context(|| {
            format!(
                "Failed to install route {} via {} on {}",
                route.subnet, route.via_ip, interface_name
            )
        })?;
    }

    Ok(())
}

async fn list_routes(interface_name: &str) -> Result<BTreeMap<String, String>> {
    let output = run_cmd_capture(
        "ip",
        &["route", "show", "dev", interface_name, "proto", "static"],
    )
    .await?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let subnet = parts.next()?.to_string();
            let mut via_ip = None;
            while let Some(part) = parts.next() {
                if part == "via" {
                    via_ip = parts.next().map(ToString::to_string);
                    break;
                }
            }
            via_ip.map(|via| (subnet, via))
        })
        .collect())
}

async fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to execute: {} {}", program, args.join(" ")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {} failed: {}", program, args.join(" "), stderr.trim())
    }
}

async fn run_cmd_capture(program: &str, args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to execute: {} {}", program, args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy_owned(output.stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {} failed: {}", program, args.join(" "), stderr.trim())
    }
}
