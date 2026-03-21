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
- `/dev/kvm`
- Rust toolchain

## Usage

```bash
# Full run (starts stack, builds image, boots cluster)
./demos/etcd-cluster/run.sh

# Tear down
./demos/etcd-cluster/run.sh --cleanup
```

## Try it out

After the cluster is ready:

```bash
# Cluster health
etcdctl endpoint health --endpoints=http://10.100.0.10:2379,http://10.100.0.11:2379,http://10.100.0.12:2379

# Write to one node, read from another
etcdctl --endpoints=http://10.100.0.10:2379 put hello world
etcdctl --endpoints=http://10.100.0.11:2379 get hello

# Kill a node — cluster survives with 2/3
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
