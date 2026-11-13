# etcd Cluster Demo

A 3-node etcd cluster, each node a qarax VM booted from an OCI image on an isolated network.

```
etcd-net   10.100.0.0/24  (bridge br-etcd, gateway 10.100.0.1)
etcd-0     10.100.0.10
etcd-1     10.100.0.11
etcd-2     10.100.0.12
this host  10.100.0.254   (veth-etcd, added by the last step)
```

The subnet is set once, as `NET_PREFIX` in `run.sh`. The node list there is
baked into the image as `CLUSTER_MEMBERS`, and each node picks its etcd name
by the static IP qarax gave it.

## What it does

1. Removes the `etcd-0..2` VMs of an earlier run
2. Builds the node image from `Containerfile` with Docker and pushes it to the local registry (`localhost:5001/etcd-node:src`)
3. Creates the `etcd-net` network if needed and pins it to a host that has `overlaybd-pool` (attached there, detached elsewhere), so all three members share its bridge
4. Imports the image into `overlaybd-pool` as the `etcd-node` storage object, which converts it to OverlayBD; an earlier run's object is replaced, since step 2 pushed a fresh build over the tag
5. Creates the three VMs with static IPs and the guest agent, attaches the image as their boot disk and starts them concurrently
6. Waits until `etcdctl endpoint health` passes for all three members (run inside `etcd-0` with `qarax vm exec`) and shows `etcdctl endpoint status`
7. Writes a key through `etcd-0` and checks that `etcd-2` reads it back
8. Plumbs a veth from this machine into `br-etcd` (with `sudo`) and checks each member's `/health` from here

Every check fails the demo if it doesn't pass. The cluster is left running.

## Prerequisites

- The local Docker Compose stack: the script starts it with `hack/run-local.sh` (same as `make run-local`) if it isn't running. `make run-local VM=1` doesn't work: the VM bridge lives in the qarax-node container's network namespace.
- Docker, `jq`, `curl`, `sudo`, `/dev/kvm`
- `cargo` (the script builds the CLI) or a `qarax` CLI on `PATH`
- `QARAX_TOKEN` matching the server's API token; it defaults to the local stack's `e2e-test-token`

## Usage

```bash
# Build, boot and check the cluster
./demos/etcd-cluster/run.sh

# Remove the VMs, the network (detached from the host first), the image object and the host veth
./demos/etcd-cluster/run.sh --cleanup
```

A re-run replaces the VMs and the image object and reuses the network.
`--cleanup` leaves the stack and the images in the local registry alone.

## Try it out

```bash
export QARAX_TOKEN=e2e-test-token

# etcdctl inside a member: no local install needed
qarax vm exec etcd-0 -- etcdctl --endpoints=http://10.100.0.10:2379,http://10.100.0.11:2379,http://10.100.0.12:2379 endpoint health
qarax vm exec etcd-1 -- etcdctl get greeting

# From this machine, over veth-etcd
curl http://10.100.0.10:2379/version

# Kill a member: the other two keep quorum
qarax vm force-stop etcd-2
qarax vm exec etcd-0 -- etcdctl put still-running yes

# etcd's log on a member's console
qarax vm console etcd-0
```

## Files

| File | Description |
|------|-------------|
| `run.sh` | Demo orchestration script |
| `Containerfile` | etcd node OCI image; its `CMD` is `etcd-node.sh` |
| `etcd-node.sh` | Finds this node's name in `CLUSTER_MEMBERS` by its IP and runs etcd |

qarax boots OverlayBD images with `qarax-init` as PID 1, which runs the image's
`CMD` with its `ENV` (there is no systemd in the guest). `qarax-init` also runs
the guest agent behind `qarax vm exec`.
