# etcd Cluster Demo

Spin up a self-contained 3-node etcd cluster, each node running as a qarax VM booted from an OCI image on an isolated network.

```
etcd-net  10.100.0.0/24
etcd-0    10.100.0.10
etcd-1    10.100.0.11
etcd-2    10.100.0.12
```

Each VM boots from the `etcd-cluster/Containerfile` image. The node determines its etcd identity at runtime from its statically assigned IP.

## Prerequisites

- Docker (with Compose)
- `podman` (to build the etcd node image)
- `jq`
- `/dev/kvm`
- Rust toolchain
- `sudo` (the last step plumbs a veth from the host into the VM bridge so the cluster is reachable from your machine)

The script starts the local stack with `hack/run-local.sh` (same as `make run-local`) if it is not already running, and uses the host registered at address `qarax-node` (override with `QARAX_HOST`). The local stack has API auth enabled; the script defaults `QARAX_TOKEN` to its `e2e-test-token`.

## Usage

```bash
# Full run (starts stack if needed, builds + converts image, boots cluster)
./demos/etcd-cluster/run.sh

# Tear down: removes the host veth and the whole local stack (make stop-local)
./demos/etcd-cluster/run.sh --cleanup
```

Re-running is safe: existing network, image object, and VMs are reused.

## Try it out

After the cluster is ready:

```bash
# Cluster health
etcdctl endpoint health --endpoints=http://10.100.0.10:2379,http://10.100.0.11:2379,http://10.100.0.12:2379

# Write to one node, read from another
etcdctl --endpoints=http://10.100.0.10:2379 put hello world
etcdctl --endpoints=http://10.100.0.11:2379 get hello

# Kill a node — cluster survives with 2/3
export QARAX_TOKEN=e2e-test-token
qarax vm stop etcd-2
etcdctl --endpoints=http://10.100.0.10:2379,http://10.100.0.11:2379 put still running yes
```

## Files

| File | Description |
|------|-------------|
| `run.sh` | Demo orchestration script |
| `Containerfile` | etcd node OCI image |
| `etcd-node.sh` | Startup script (maps IP → etcd node name, launches etcd) |
| `etcd-node.service` | systemd unit that runs `etcd-node.sh` at boot |
