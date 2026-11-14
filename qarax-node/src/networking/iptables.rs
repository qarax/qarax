use std::collections::HashSet;

use anyhow::{Context, Result};
use tracing::info;

const FORWARD_CHAIN: &str = "FORWARD";

/// Set up NAT masquerade and forwarding rules for a bridge subnet. Rules that
/// are already present aren't added again, so re-attaching a network doesn't
/// pile up duplicates that a later teardown (one `-D` each) would leave behind.
pub async fn setup_nat(bridge: &str, subnet: &str) -> Result<()> {
    super::validate_iface_name(bridge)?;
    super::validate_ipv4_cidr(subnet)?;
    info!("Setting up NAT for bridge {} subnet {}", bridge, subnet);

    // Enable IP forwarding via /proc (sysctl binary may not be present)
    tokio::fs::write("/proc/sys/net/ipv4/ip_forward", b"1")
        .await
        .context("Failed to enable ip_forward")?;

    ensure_rule(
        &["-t", "nat"],
        "POSTROUTING",
        &["-s", subnet, "!", "-o", bridge, "-j", "MASQUERADE"],
    )
    .await?;
    ensure_rule(&[], FORWARD_CHAIN, &["-i", bridge, "-j", "ACCEPT"]).await?;
    ensure_rule(&[], FORWARD_CHAIN, &["-o", bridge, "-j", "ACCEPT"]).await?;

    Ok(())
}

/// Append an iptables rule unless `-C` finds it already in the chain.
async fn ensure_rule(table: &[&str], chain: &str, rule: &[&str]) -> Result<()> {
    let with = |op: &'static str| -> Vec<&str> {
        let mut args = table.to_vec();
        args.push(op);
        args.push(chain);
        args.extend_from_slice(rule);
        args
    };
    if run_cmd("iptables", &with("-C")).await.is_ok() {
        return Ok(());
    }
    run_cmd("iptables", &with("-A")).await
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
    let _ = remove_nat_exemption_rules(bridge, subnet).await;

    Ok(())
}

pub async fn sync_network_isolation(
    bridge: &str,
    local_subnet: &str,
    blocked_subnets: &[String],
    nat_exempt_subnets: &[String],
) -> Result<()> {
    super::validate_iface_name(bridge)?;
    super::validate_ipv4_cidr(local_subnet)?;
    for subnet in blocked_subnets {
        super::validate_ipv4_cidr(subnet)?;
    }
    for subnet in nat_exempt_subnets {
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
    sync_nat_exemption_rules(bridge, local_subnet, nat_exempt_subnets).await?;
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

async fn ensure_nat_exemption_rule(
    bridge: &str,
    local_subnet: &str,
    exempt_subnet: &str,
) -> Result<()> {
    let rule = [
        "-s",
        local_subnet,
        "-d",
        exempt_subnet,
        "!",
        "-o",
        bridge,
        "-j",
        "RETURN",
    ];

    let mut check_args = vec!["-t", "nat", "-C", "POSTROUTING"];
    check_args.extend_from_slice(&rule);
    if run_cmd("iptables", &check_args).await.is_ok() {
        return Ok(());
    }

    let mut insert_args = vec!["-t", "nat", "-I", "POSTROUTING", "1"];
    insert_args.extend_from_slice(&rule);
    run_cmd("iptables", &insert_args).await
}

async fn sync_nat_exemption_rules(
    bridge: &str,
    local_subnet: &str,
    nat_exempt_subnets: &[String],
) -> Result<()> {
    for subnet in nat_exempt_subnets {
        ensure_nat_exemption_rule(bridge, local_subnet, subnet).await?;
    }

    let desired_rules: Vec<Vec<String>> = nat_exempt_subnets
        .iter()
        .map(|subnet| {
            vec![
                "-s".to_string(),
                local_subnet.to_string(),
                "-d".to_string(),
                subnet.clone(),
                "!".to_string(),
                "-o".to_string(),
                bridge.to_string(),
                "-j".to_string(),
                "RETURN".to_string(),
            ]
        })
        .collect();

    let rules = run_cmd_capture("iptables", &["-t", "nat", "-S", "POSTROUTING"]).await?;
    for line in rules.lines().filter(|line| {
        line.contains(&format!("-s {local_subnet}"))
            && line.contains(&format!("! -o {bridge}"))
            && line.contains("-j RETURN")
    }) {
        if desired_rules
            .iter()
            .any(|desired_rule| rule_matches(line, desired_rule))
        {
            continue;
        }

        let mut parts: Vec<String> = line.split_whitespace().map(ToString::to_string).collect();
        if parts.first().is_some_and(|part| part == "-A") {
            parts[0] = "-D".to_string();
            parts.insert(0, "nat".to_string());
            parts.insert(0, "-t".to_string());
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            run_cmd("iptables", &refs).await?;
        }
    }

    Ok(())
}

fn rule_matches(line: &str, desired_rule: &[String]) -> bool {
    let parts: HashSet<&str> = line.split_whitespace().collect();
    desired_rule
        .iter()
        .all(|token| parts.contains(token.as_str()))
}

async fn remove_nat_exemption_rules(bridge: &str, local_subnet: &str) -> Result<()> {
    let rules = run_cmd_capture("iptables", &["-t", "nat", "-S", "POSTROUTING"]).await?;
    for line in rules.lines().filter(|line| {
        line.contains(&format!("-s {local_subnet}"))
            && line.contains(&format!("! -o {bridge}"))
            && line.contains("-j RETURN")
    }) {
        let mut parts: Vec<String> = line.split_whitespace().map(ToString::to_string).collect();
        if parts.first().is_some_and(|part| part == "-A") {
            parts[0] = "-D".to_string();
            parts.insert(0, "nat".to_string());
            parts.insert(0, "-t".to_string());
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            let _ = run_cmd("iptables", &refs).await;
        }
    }
    Ok(())
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
        Ok(String::from_utf8_lossy_owned(output.stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {} failed: {}", program, args.join(" "), stderr.trim())
    }
}
