# Network Isolation Demo

Demonstrates VPC-scoped managed networks and VM security groups end to end:

- create two managed networks in one VPC and a third network in another VPC
- create one VM per network
- prove same-VPC routing between the two subnets (VM A pings VM B)
- prove VPC isolation: VM A cannot reach VM C, whose network is in the other
  VPC on the same host
- attach an empty security group to VM B and watch ingress become blocked
- add an ICMP rule and watch connectivity return without restarting either VM

With `--cross-host`, network A (VMs A and C) is attached to one host and network
B (VM B) to a second host, so the same-VPC traffic crosses hosts.

## Prerequisites

- local stack from `make run-local` (the script starts it via
  `hack/run-local.sh` if the API is not reachable). Guests are reached over SSH
  through the `qarax-node` container, so `make run-local VM=1` is not supported.
- for `--cross-host`, a second UP host: on the local stack the script starts
  the `qarax-node-2` container and registers it as `local-node-2` (with
  `e2e/setup_host.sh`, which needs `python3`)
- `jq`, Docker
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/network-isolation/run.sh

# networks A and B on different hosts
./demos/network-isolation/run.sh --cross-host

# keep the VMs, networks and security group for inspection
KEEP=1 ./demos/network-isolation/run.sh

# remove what a KEEP=1 or interrupted run left behind
./demos/network-isolation/run.sh --cleanup

# target a different API endpoint
./demos/network-isolation/run.sh --server http://localhost:8000
```

## Layout

| Network | VPC | Subnet | Host | VM |
|---------|-----|--------|------|----|
| `demo-netiso-a` | `demo-netiso-vpc` | `10.121.1.0/24` | host A | `demo-netiso-vm-a` (`10.121.1.10`) |
| `demo-netiso-b` | `demo-netiso-vpc` | `10.121.2.0/24` | host A, or host B with `--cross-host` | `demo-netiso-vm-b` (`10.121.2.10`) |
| `demo-netiso-c` | `demo-netiso-other-vpc` | `10.122.1.0/24` | host A | `demo-netiso-vm-c` (`10.122.1.10`) |

The script checks that each VM was scheduled onto the host its network is
attached to.

## How it proves the feature

The demo SSHes into VM A from the `qarax-node` container on host A (guest
subnets are only routable there) and runs `ping` inside the guest:

1. VM A -> VM B succeeds: same VPC, different subnets (and hosts, with `--cross-host`)
2. VM A -> VM C gets 100% packet loss, while VM C itself answers SSH from its
   host: different VPC
3. after an empty security group is attached to VM B, VM A -> VM B gets 100%
   packet loss
4. after an ICMP ingress rule for subnet A is added, VM A -> VM B succeeds
   again, without restarting either VM

A blocked ping only counts when ping ran inside the guest and reported 100%
packet loss, so an SSH failure can't pass as isolation.

## How routing and isolation work

- **Same VPC, one host:** each network gets its own bridge on the host, with
  the gateway address and a DHCP server. The host routes between the bridges.
- **Same VPC, several hosts:** when a VPC's networks are attached to more than
  one host, qarax builds a VXLAN overlay between those hosts (one interface per
  VPC, VNI derived from the VPC name) and installs a route on each host for
  every remote subnet in the VPC, via the peer's overlay address. VMs keep
  their per-subnet gateway; the overlay only carries traffic between hosts.
- **Different VPCs:** for every bridge of a network in a VPC, qarax installs
  forward rules on the host that drop traffic to and from the subnets of
  networks in other VPCs (or in no VPC), wherever those networks are attached.
  So VM A and VM C share a host, yet the host drops their traffic.
- **Security groups** are enforced on the host at each VM's port on the
  bridge, so they also apply between VMs on the same network. A VM with a
  security group but no ingress rules gets default-deny ingress. Rule changes
  are synced to the host immediately, and before a VM boots. Every VM NIC may
  also only send from its own MAC and address.

### Why the networks use `--network-type isolated`

Each network is a host bridge whose outbound traffic is masqueraded (NAT).
Between subnets of the same VPC, qarax exempts `isolated` networks from that
NAT, so VM B sees packets from VM A's own address (`10.121.1.10`). That is what
lets the ICMP rule for `10.121.1.0/24` match. With the default type `bridge`,
traffic leaving host A over the overlay is masqueraded to host A's overlay
address, and VM B's security group never sees subnet A as the source. On a
single host the rule matches either way (security-group rules match the
source from before the host's NAT), but the demo uses `isolated` in both
modes so the same rule works across hosts.

## Cleanup

At exit the script deletes the VMs, the security group and the networks
(detaching each network from its hosts first, which removes the bridge, DHCP
server and NAT rules on the node). At start it removes anything named
`demo-netiso-*` that an earlier run left, so a re-run always works. If another
network already uses one of the demo's subnets, the script stops and names it.

With `KEEP=1` nothing is deleted at exit; run the script with `--cleanup` to
remove the resources afterwards.
