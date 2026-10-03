# Hyperconverged Demo

Run the qarax control plane (API + PostgreSQL) inside a Cloud Hypervisor VM on bare metal, with the same VM also running `qarax-node` — the "hosted engine" pattern.

```
Host (bare metal)
├── passt (vhost-user networking, forwards 8000/3000/2222 → VM)
├── Local OCI registry (podman, port 5000)
└── Cloud Hypervisor VM: control-plane (192.168.100.10)
    ├── qarax API (port 8000)
    ├── qarax-node (port 50051)
    ├── Grafana/otel-lgtm (port 3000)
    ├── overlaybd-tcmu
    └── PostgreSQL (local)
```

## Prerequisites

- Linux host with KVM and nested KVM (`kvm_intel.nested=Y`), read/write access to `/dev/kvm`
- Rust toolchain with the `x86_64-unknown-linux-musl` target
- `podman`, `passt`, `guestfish` (libguestfs-tools), `python3`
- `cloud-hypervisor` on PATH (auto-downloaded if missing)
- Host ports 5000, 8000, 3000 and 2222 free (override with `REGISTRY_PORT`,
  `API_HOST_PORT`, `GRAFANA_HOST_PORT`, `CP_SSH_PORT`)

The control plane runs with the production config, which requires an API
token: it is set from `QARAX_TOKEN` (default `e2e-test-token`, see
`demos/lib.sh`) — set your own before exposing the API on a LAN.

## Usage

Run as root or as a user with `/dev/kvm` access; use the same user for
`--cleanup`.

```bash
# Full build + run (defaults to passt-backed workload VM networking)
./demos/hyperconverged/run.sh

# Skip the server/node cargo build (use existing release binaries)
SKIP_BUILD=1 ./demos/hyperconverged/run.sh

# With optional extras
./demos/hyperconverged/run.sh --with-local         # also create a local storage pool
./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export
./demos/hyperconverged/run.sh --with-local-vm      # boot a firmware VM from a Fedora cloud image (implies --with-local)
./demos/hyperconverged/run.sh --with-db-vm         # boot an OCI PostgreSQL VM
./demos/hyperconverged/run.sh --network-backend bridge

# Overrides: --db-image IMAGE, --local-pool-path PATH, --cloud-image-url URL

# Tear down
./demos/hyperconverged/run.sh --cleanup
```

Each run rebuilds the control-plane disk from scratch, so run `--cleanup`
before starting again.

## After startup

```bash
export QARAX_SERVER=http://localhost:8000
export QARAX_TOKEN=e2e-test-token   # or whatever you ran the demo with
qarax vm list
qarax vm attach alpine-vm
```

Grafana is at http://localhost:3000 (admin/admin) with the Qarax demo
dashboards imported. SSH into the control-plane VM with
`ssh -p 2222 root@localhost` (password: `qarax`).

`passt` is the default backend for workload VMs in this demo because it avoids
per-network bridge, DHCP, and NAT setup inside the nested node. Use
`--network-backend bridge` if you specifically want bridged Qarax-managed guest
networking instead.

## Files

| File | Description |
|------|-------------|
| `run.sh` | Demo orchestration script |
| `Containerfile.control-plane` | OCI image for the control plane VM (qarax + qarax-node + PostgreSQL) |
| `grafana/*.json` | Dashboards imported into the in-VM Grafana |
