# Network Isolation Demo

Demonstrates Qarax network isolation end to end on the local stack:

- create two managed networks in the same VPC
- attach both networks to one host
- create one VM per subnet
- prove same-VPC cross-subnet routing works
- attach an empty security group to one VM and watch ingress become blocked
- add an ICMP rule and watch connectivity return without restarting the VM

## Prerequisites

- local stack from `make run-local` (the script starts it via
  `hack/run-local.sh` if the API is not reachable)
- `jq`
- Docker with `docker compose`
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/network-isolation/run.sh

# keep the resources around for inspection
./demos/network-isolation/run.sh --keep-resources

# target a different API endpoint
./demos/network-isolation/run.sh --server http://localhost:8000
```

## How it proves the feature

The demo uses the local `qarax-node` container as the SSH hop into the first VM,
then runs `ping` from that VM to the second VM:

1. ping succeeds while both VMs are on different subnets in the same VPC
2. an empty security group is attached to VM B, and ping starts failing
3. an ICMP ingress rule for subnet A is added, and ping succeeds again

That final step happens without restarting either VM, showing live firewall sync
on `qarax-node`.

## Cleanup

By default the script cleans up both VMs, both networks, and the security group
on exit.

Use `--keep-resources` if you want to inspect them afterward. Delete the kept
networks before re-running: the demo always uses subnets `10.121.1.0/24` and
`10.121.2.0/24` and refuses to start if a network already uses them.
