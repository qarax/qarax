# Cross-Host VPC Security Group Demo

Demonstrates Qarax cross-host VPC routing end to end on a live two-node stack:

- create two isolated networks in one VPC
- attach network A to host 1 and network B to host 2
- create one VM per network and prove they land on different hosts
- prove cross-host same-VPC connectivity works
- attach an empty security group to VM B and watch ingress become blocked
- add an ICMP rule for subnet A and watch connectivity return without restarting either VM

## Prerequisites

- a live two-node Qarax stack with both hosts `up`
- `jq`
- Docker with `docker compose`
- `qarax` CLI on PATH, or a Rust toolchain so the demo can build it

The intended environment is the two-node e2e stack, for example:

```bash
cd e2e
REBUILD=1 KEEP=1 ./run_e2e_tests.sh test_network.py -k vpc_routing_and_security_group_updates
```

## Usage

```bash
./demos/cross-host-vpc/run.sh

# keep the resources around for inspection
./demos/cross-host-vpc/run.sh --keep-resources

# target a different API endpoint
./demos/cross-host-vpc/run.sh --server http://localhost:8000
```

## How it proves the feature

The demo uses the primary `qarax-node` container as the SSH hop into VM A,
then runs `ping` from VM A to VM B on the other host:

1. ping succeeds while the VMs live on different hosts but share one VPC
2. an empty security group is attached to VM B, and ping starts failing
3. an ICMP ingress rule for subnet A is added, and ping succeeds again

That final step happens without restarting either VM, proving the live
security-group update path and the same-VPC cross-host source-IP handling fix.

## Cleanup

By default the script cleans up both VMs, both networks, and the security group
on exit.

Use `--keep-resources` if you want to inspect them afterward.
