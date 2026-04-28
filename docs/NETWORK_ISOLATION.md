# Network Isolation Guide

This guide covers the network-isolation features added on top of Qarax managed
networks:

- **VPC-style subnet grouping** with `vpc_name`
- **Security groups** attached to VMs

These features are designed to work with Qarax-managed networks and managed VM
NICs. They do not introduce a separate fabric or overlay network.

## Mental model

- A **network** is a managed subnet in Qarax.
- A **VPC** is an optional label on a network. Networks with the same VPC name
  can route between their subnets **when attached to the same host**.
- A **security group** is a reusable set of firewall rules attached to a VM.

In practice:

- same VPC name + same host => cross-subnet routing allowed
- different VPC names => cross-subnet traffic blocked
- no VPC name => legacy behavior stays in place

## Scope and limits

Current behavior is intentionally narrow:

- VPC routing/isolation is enforced for managed subnets attached to the **same
  host**
- security groups apply to **managed routed traffic** on managed NICs
- empty security groups still take effect and give you **default-deny ingress**
- if a VM has no explicit egress rules, egress remains allowed
- networks using passt/manual unmanaged paths are outside this enforcement path

## Quick start

### 1. Create two managed networks in the same VPC

```bash
qarax network create \
  --name app-a \
  --subnet 10.10.1.0/24 \
  --gateway 10.10.1.1 \
  --vpc demo

qarax network create \
  --name app-b \
  --subnet 10.10.2.0/24 \
  --gateway 10.10.2.1 \
  --vpc demo
```

### 2. Attach both networks to the same host

```bash
qarax network attach-host --network app-a --host local-node --bridge-name qappa
qarax network attach-host --network app-b --host local-node --bridge-name qappb
```

This is what creates the bridge/DHCP/NAT state on the host and gives Qarax a
place to enforce isolation.

### 3. Create a security group

```bash
qarax security-group create --name web --description "allow app subnet access"
```

### 4. Add rules

Allow SSH and ICMP from subnet `10.10.1.0/24`:

```bash
qarax security-group add-rule \
  --security-group web \
  --direction ingress \
  --protocol tcp \
  --cidr 10.10.1.0/24 \
  --port-start 22 \
  --port-end 22

qarax security-group add-rule \
  --security-group web \
  --direction ingress \
  --protocol icmp \
  --cidr 10.10.1.0/24
```

Rule fields:

- `--direction`: `ingress` or `egress`
- `--protocol`: `any`, `tcp`, `udp`, or `icmp`
- `--cidr`: optional source/destination CIDR
- `--port-start` / `--port-end`: for TCP/UDP rules

### 5. Create VMs and bind security groups

Bind at create time:

```bash
qarax vm create --name vm-a --network app-a
qarax vm create --name vm-b --network app-b --security-group web
```

Or attach later:

```bash
qarax vm attach-security-group vm-b --security-group web
```

Attaching an empty security group is valid. It immediately applies a
default-deny ingress policy until you add rules.

## Common workflows

### List networks and verify VPC membership

```bash
qarax network list
qarax network get app-a
```

### Inspect security groups

```bash
qarax security-group list
qarax security-group get web
qarax security-group list-rules web
```

### Inspect VM bindings

```bash
qarax vm list-security-groups vm-b
```

### Remove a rule or detach a group

```bash
qarax security-group delete-rule --security-group web --rule-id <rule-uuid>
qarax vm detach-security-group vm-b --security-group web
```

Changes are synced live to the host. You do not need to recreate the VM to
apply updated rules.

## Behavior details

### VPC behavior

When Qarax computes host isolation for an attached network:

- if the network has a `vpc_name`, traffic to attached networks with a
  **different** `vpc_name` is blocked
- traffic to attached networks with the **same** `vpc_name` is allowed
- if the network has **no** `vpc_name`, Qarax does not add the new VPC-specific
  isolation rules

This preserves backward compatibility for existing managed networks.

### Security-group behavior

Security groups are attached to the **VM**, not the network.

Qarax resolves the VM's managed NICs and pushes firewall state down to the host.
That sync happens when:

- the VM starts
- a NIC is added or removed
- a security group is attached or detached
- a security-group rule is created or deleted

## Example: isolate environments on one host

Put production subnets in one VPC and staging subnets in another:

```bash
qarax network create --name prod-a --subnet 10.20.1.0/24 --gateway 10.20.1.1 --vpc prod
qarax network create --name prod-b --subnet 10.20.2.0/24 --gateway 10.20.2.1 --vpc prod
qarax network create --name stage-a --subnet 10.30.1.0/24 --gateway 10.30.1.1 --vpc stage
```

Result on the same host:

- `prod-a` <-> `prod-b`: allowed
- `prod-a` <-> `stage-a`: blocked

Then use security groups to further restrict which VMs in `prod` may accept
SSH, app traffic, or ICMP.

## Cleanup

```bash
qarax vm detach-security-group vm-b --security-group web
qarax security-group delete web

qarax network detach-host --network app-a --host local-node
qarax network detach-host --network app-b --host local-node

qarax network delete app-a
qarax network delete app-b
```
