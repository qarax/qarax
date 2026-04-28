use std::collections::HashSet;

use anyhow::{Context, Result};
use tracing::info;

use crate::rpc::node::{FirewallDirection, FirewallProtocol, VmFirewallInterface};

const FORWARD_CHAIN: &str = "FORWARD";

struct VmFirewallChains {
    out_parent: String,
    in_parent: String,
    out_a: String,
    out_b: String,
    in_a: String,
    in_b: String,
}

impl VmFirewallChains {
    fn new(vm_id: &str) -> Self {
        let suffix = sanitize_suffix(vm_id, 21);
        Self {
            out_parent: format!("QXVMO{suffix}"),
            in_parent: format!("QXVMI{suffix}"),
            out_a: format!("QXVOA{suffix}"),
            out_b: format!("QXVOB{suffix}"),
            in_a: format!("QXVIA{suffix}"),
            in_b: format!("QXVIB{suffix}"),
        }
    }
}

/// Set up NAT masquerade and forwarding rules for a bridge subnet.
pub async fn setup_nat(bridge: &str, subnet: &str) -> Result<()> {
    super::validate_iface_name(bridge)?;
    super::validate_ipv4_cidr(subnet)?;
    info!("Setting up NAT for bridge {} subnet {}", bridge, subnet);

    // Enable IP forwarding via /proc (sysctl binary may not be present)
    tokio::fs::write("/proc/sys/net/ipv4/ip_forward", b"1")
        .await
        .context("Failed to enable ip_forward")?;

    let _ = run_cmd(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "POSTROUTING",
            "-s",
            subnet,
            "!",
            "-o",
            bridge,
            "-j",
            "MASQUERADE",
        ],
    )
    .await;
    let _ = run_cmd(
        "iptables",
        &["-A", FORWARD_CHAIN, "-i", bridge, "-j", "ACCEPT"],
    )
    .await;
    let _ = run_cmd(
        "iptables",
        &["-A", FORWARD_CHAIN, "-o", bridge, "-j", "ACCEPT"],
    )
    .await;

    Ok(())
}

/// Remove NAT and forwarding rules for a bridge subnet.
pub async fn teardown_nat(bridge: &str, subnet: &str) -> Result<()> {
    super::validate_iface_name(bridge)?;
    super::validate_ipv4_cidr(subnet)?;
    info!("Tearing down NAT for bridge {} subnet {}", bridge, subnet);

    let _ = run_cmd(
        "iptables",
        &[
            "-t",
            "nat",
            "-D",
            "POSTROUTING",
            "-s",
            subnet,
            "!",
            "-o",
            bridge,
            "-j",
            "MASQUERADE",
        ],
    )
    .await;
    let _ = run_cmd(
        "iptables",
        &["-D", FORWARD_CHAIN, "-i", bridge, "-j", "ACCEPT"],
    )
    .await;
    let _ = run_cmd(
        "iptables",
        &["-D", FORWARD_CHAIN, "-o", bridge, "-j", "ACCEPT"],
    )
    .await;

    Ok(())
}

pub async fn sync_network_isolation(bridge: &str, blocked_subnets: &[String]) -> Result<()> {
    super::validate_iface_name(bridge)?;
    for subnet in blocked_subnets {
        super::validate_ipv4_cidr(subnet)?;
    }

    let out_chain = format!("QXNETO{}", sanitize_suffix(bridge, 20));
    let in_chain = format!("QXNETI{}", sanitize_suffix(bridge, 20));

    ensure_chain(&out_chain).await?;
    ensure_chain(&in_chain).await?;
    ensure_jump_rule(FORWARD_CHAIN, &["-i", bridge, "-j", &out_chain]).await?;
    ensure_jump_rule(FORWARD_CHAIN, &["-o", bridge, "-j", &in_chain]).await?;

    flush_chain(&out_chain).await?;
    flush_chain(&in_chain).await?;

    for subnet in blocked_subnets {
        run_cmd("iptables", &["-A", &out_chain, "-d", subnet, "-j", "DROP"]).await?;
        run_cmd("iptables", &["-A", &in_chain, "-s", subnet, "-j", "DROP"]).await?;
    }

    run_cmd("iptables", &["-A", &out_chain, "-j", "RETURN"]).await?;
    run_cmd("iptables", &["-A", &in_chain, "-j", "RETURN"]).await?;
    Ok(())
}

pub async fn teardown_network_isolation(bridge: &str) -> Result<()> {
    super::validate_iface_name(bridge)?;
    let out_chain = format!("QXNETO{}", sanitize_suffix(bridge, 20));
    let in_chain = format!("QXNETI{}", sanitize_suffix(bridge, 20));
    remove_forward_rules_referencing(&out_chain).await?;
    remove_forward_rules_referencing(&in_chain).await?;
    delete_chain_if_exists(&out_chain).await?;
    delete_chain_if_exists(&in_chain).await?;
    Ok(())
}

pub async fn sync_vm_firewall(vm_id: &str, interfaces: &[VmFirewallInterface]) -> Result<()> {
    if interfaces.is_empty() {
        teardown_vm_firewall(vm_id).await?;
        return Ok(());
    }

    let chains = VmFirewallChains::new(vm_id);
    ensure_chain(&chains.out_parent).await?;
    ensure_chain(&chains.in_parent).await?;

    let next_out_chain = next_child_chain(&chains.out_parent, &chains.out_a, &chains.out_b).await?;
    let next_in_chain = next_child_chain(&chains.in_parent, &chains.in_a, &chains.in_b).await?;

    ensure_chain(&next_out_chain).await?;
    ensure_chain(&next_in_chain).await?;
    flush_chain(&next_out_chain).await?;
    flush_chain(&next_in_chain).await?;

    run_cmd(
        "iptables",
        &[
            "-A",
            &next_in_chain,
            "-m",
            "conntrack",
            "--ctstate",
            "ESTABLISHED,RELATED",
            "-j",
            "ACCEPT",
        ],
    )
    .await?;
    run_cmd(
        "iptables",
        &[
            "-A",
            &next_out_chain,
            "-m",
            "conntrack",
            "--ctstate",
            "ESTABLISHED,RELATED",
            "-j",
            "ACCEPT",
        ],
    )
    .await?;

    let mut has_egress_rules = false;
    let mut desired_in_jump_rules = Vec::with_capacity(interfaces.len());
    let mut desired_out_jump_rules = Vec::with_capacity(interfaces.len());

    for iface in interfaces {
        super::validate_iface_name(&iface.bridge_name)?;
        super::validate_ipv4_address(&iface.ip)?;

        desired_in_jump_rules.push(vec![
            "-o".to_string(),
            iface.bridge_name.clone(),
            "-d".to_string(),
            format!("{}/32", iface.ip),
            "-j".to_string(),
            chains.in_parent.clone(),
        ]);
        desired_out_jump_rules.push(vec![
            "-i".to_string(),
            iface.bridge_name.clone(),
            "-s".to_string(),
            format!("{}/32", iface.ip),
            "-j".to_string(),
            chains.out_parent.clone(),
        ]);

        for rule in &iface.rules {
            let direction =
                FirewallDirection::try_from(rule.direction).unwrap_or(FirewallDirection::Ingress);
            let protocol =
                FirewallProtocol::try_from(rule.protocol).unwrap_or(FirewallProtocol::Any);
            let cidr = (!rule.cidr.is_empty()).then_some(rule.cidr.as_str());
            if let Some(cidr) = cidr {
                super::validate_ipv4_cidr(cidr)?;
            }

            let (chain, cidr_flag, ports_allowed) = match direction {
                FirewallDirection::Ingress => (&next_in_chain, "-s", true),
                FirewallDirection::Egress => {
                    has_egress_rules = true;
                    (&next_out_chain, "-d", true)
                }
            };

            let mut args = vec!["-A".to_string(), chain.to_string()];
            if let Some(cidr) = cidr {
                args.push(cidr_flag.to_string());
                args.push(cidr.to_string());
            }

            match protocol {
                FirewallProtocol::Any => {}
                FirewallProtocol::Tcp => {
                    args.push("-p".to_string());
                    args.push("tcp".to_string());
                }
                FirewallProtocol::Udp => {
                    args.push("-p".to_string());
                    args.push("udp".to_string());
                }
                FirewallProtocol::Icmp => {
                    args.push("-p".to_string());
                    args.push("icmp".to_string());
                }
            }

            if ports_allowed
                && matches!(protocol, FirewallProtocol::Tcp | FirewallProtocol::Udp)
                && let (Some(start), Some(end)) = (rule.port_start, rule.port_end)
            {
                let port_range = if start == end {
                    start.to_string()
                } else {
                    format!("{start}:{end}")
                };
                args.push("--dport".to_string());
                args.push(port_range);
            }

            args.push("-j".to_string());
            args.push("ACCEPT".to_string());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_cmd("iptables", &refs).await?;
        }
    }

    run_cmd("iptables", &["-A", &next_in_chain, "-j", "DROP"]).await?;

    if has_egress_rules {
        run_cmd("iptables", &["-A", &next_out_chain, "-j", "DROP"]).await?;
    } else {
        run_cmd("iptables", &["-A", &next_out_chain, "-j", "ACCEPT"]).await?;
    }

    point_parent_to_child(&chains.in_parent, &next_in_chain).await?;
    point_parent_to_child(&chains.out_parent, &next_out_chain).await?;
    sync_forward_jump_rules(&chains.in_parent, &desired_in_jump_rules).await?;
    sync_forward_jump_rules(&chains.out_parent, &desired_out_jump_rules).await?;

    let stale_in_chain = if next_in_chain == chains.in_a {
        chains.in_b.as_str()
    } else {
        chains.in_a.as_str()
    };
    let stale_out_chain = if next_out_chain == chains.out_a {
        chains.out_b.as_str()
    } else {
        chains.out_a.as_str()
    };
    delete_chain_if_exists(stale_in_chain).await?;
    delete_chain_if_exists(stale_out_chain).await?;

    Ok(())
}

pub async fn teardown_vm_firewall(vm_id: &str) -> Result<()> {
    let chains = VmFirewallChains::new(vm_id);
    remove_forward_rules_referencing(&chains.out_parent).await?;
    remove_forward_rules_referencing(&chains.in_parent).await?;
    delete_chain_if_exists(&chains.out_parent).await?;
    delete_chain_if_exists(&chains.in_parent).await?;
    delete_chain_if_exists(&chains.out_a).await?;
    delete_chain_if_exists(&chains.out_b).await?;
    delete_chain_if_exists(&chains.in_a).await?;
    delete_chain_if_exists(&chains.in_b).await?;
    Ok(())
}

fn sanitize_suffix(input: &str, max_len: usize) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(max_len)
        .collect::<String>()
}

async fn ensure_chain(chain: &str) -> Result<()> {
    match run_cmd("iptables", &["-N", chain]).await {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("Chain already exists") => Ok(()),
        Err(error) => Err(error),
    }
}

async fn flush_chain(chain: &str) -> Result<()> {
    run_cmd("iptables", &["-F", chain]).await
}

async fn ensure_jump_rule(parent_chain: &str, rule: &[&str]) -> Result<()> {
    let mut check_args = vec!["-C", parent_chain];
    check_args.extend_from_slice(rule);
    if run_cmd("iptables", &check_args).await.is_ok() {
        return Ok(());
    }

    let mut insert_args = vec!["-I", parent_chain, "1"];
    insert_args.extend_from_slice(rule);
    run_cmd("iptables", &insert_args).await
}

async fn next_child_chain(parent_chain: &str, child_a: &str, child_b: &str) -> Result<String> {
    let rules = run_cmd_capture("iptables", &["-S", parent_chain]).await?;
    if rules
        .lines()
        .any(|line| line == format!("-A {parent_chain} -j {child_a}"))
    {
        Ok(child_b.to_string())
    } else {
        Ok(child_a.to_string())
    }
}

async fn point_parent_to_child(parent_chain: &str, child_chain: &str) -> Result<()> {
    let rules = run_cmd_capture("iptables", &["-S", parent_chain]).await?;
    if rules
        .lines()
        .any(|line| line.starts_with(&format!("-A {parent_chain} ")))
    {
        run_cmd("iptables", &["-R", parent_chain, "1", "-j", child_chain]).await?;
        while run_cmd("iptables", &["-D", parent_chain, "2"])
            .await
            .is_ok()
        {}
    } else {
        run_cmd("iptables", &["-A", parent_chain, "-j", child_chain]).await?;
    }
    Ok(())
}

async fn sync_forward_jump_rules(parent_chain: &str, desired_rules: &[Vec<String>]) -> Result<()> {
    for rule in desired_rules {
        let refs: Vec<&str> = rule.iter().map(String::as_str).collect();
        ensure_jump_rule(FORWARD_CHAIN, &refs).await?;
    }

    let rules = run_cmd_capture("iptables", &["-S", FORWARD_CHAIN]).await?;
    for line in rules
        .lines()
        .filter(|line| line.contains(&format!("-j {parent_chain}")))
    {
        if desired_rules
            .iter()
            .any(|desired_rule| forward_rule_matches(line, desired_rule))
        {
            continue;
        }

        let mut parts: Vec<String> = line.split_whitespace().map(ToString::to_string).collect();
        if parts.first().is_some_and(|part| part == "-A") {
            parts[0] = "-D".to_string();
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            run_cmd("iptables", &refs).await?;
        }
    }

    Ok(())
}

fn forward_rule_matches(line: &str, desired_rule: &[String]) -> bool {
    let parts: HashSet<&str> = line.split_whitespace().collect();
    desired_rule
        .iter()
        .all(|token| parts.contains(token.as_str()))
}

async fn remove_forward_rules_referencing(chain: &str) -> Result<()> {
    let rules = run_cmd_capture("iptables", &["-S", FORWARD_CHAIN]).await?;
    for line in rules
        .lines()
        .filter(|line| line.contains(&format!("-j {chain}")))
    {
        let mut parts: Vec<String> = line.split_whitespace().map(ToString::to_string).collect();
        if parts.first().is_some_and(|part| part == "-A") {
            parts[0] = "-D".to_string();
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            let _ = run_cmd("iptables", &refs).await;
        }
    }

    let _ = run_cmd("iptables", &["-F", chain]).await;
    let _ = run_cmd("iptables", &["-X", chain]).await;
    Ok(())
}

async fn delete_chain_if_exists(chain: &str) -> Result<()> {
    if !chain_exists(chain).await? {
        return Ok(());
    }

    let _ = run_cmd("iptables", &["-F", chain]).await;
    let _ = run_cmd("iptables", &["-X", chain]).await;
    Ok(())
}

async fn chain_exists(chain: &str) -> Result<bool> {
    let output = run_cmd_capture("iptables", &["-S"]).await?;
    Ok(output.lines().any(|line| line == format!("-N {chain}")))
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
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {} failed: {}", program, args.join(" "), stderr.trim())
    }
}
