# Hyperconverged Demo

Run the qarax control plane (API + PostgreSQL) inside a Cloud Hypervisor VM on bare metal, with the same VM also running `qarax-node` — the "hosted engine" pattern.

```
Host (bare metal)
├── passt (vhost-user networking, forwards 8000/3000/2222 → VM)
├── Local OCI registry (podman, port 5000; the VM reaches it at 192.168.100.1)
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
- `podman`, `passt`, `guestfish` (libguestfs-tools), `qemu-img`, `curl`, `jq`,
  and `python3` (used by `e2e/setup_host.sh` to register the host)
- Host ports 5000, 8000, 3000 and 2222 free (override with `REGISTRY_PORT`,
  `API_HOST_PORT`, `GRAFANA_HOST_PORT`, `CP_SSH_PORT`)

The script downloads the Cloud Hypervisor release pinned in
`versions/cloud-hypervisor-version` and `hypervisor-fw` itself.

The control plane runs with the production config, which requires an API
token: it is set from `QARAX_TOKEN` (default `e2e-test-token`, see
`demos/lib.sh`) — set your own before exposing the API on a LAN.

## Usage

Run as root or as a user with `/dev/kvm` access; use the same user for
`--cleanup`.

```bash
# Build + run (defaults to passt-backed workload VM networking)
./demos/hyperconverged/run.sh

# Skip the cargo builds (use existing release binaries and CLI)
SKIP_BUILD=1 ./demos/hyperconverged/run.sh

# With optional extras
./demos/hyperconverged/run.sh --with-local         # also create a local storage pool
./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export
./demos/hyperconverged/run.sh --with-local-vm      # boot a firmware VM from a Fedora cloud image (implies --with-local)
./demos/hyperconverged/run.sh --with-db-vm         # boot an OCI PostgreSQL VM
./demos/hyperconverged/run.sh --network-backend bridge

# Overrides: --db-image IMAGE, --local-pool-path PATH, --cloud-image-url URL,
# DEMO_IMAGE (the OCI image for alpine-vm)

# Stop the control-plane VM, passt and the registry
./demos/hyperconverged/run.sh --cleanup
```

A successful run leaves the control-plane VM running for you to use. Running
the script again replaces it: every run boots a fresh control plane (empty
database, no VMs), so there is no need to run `--cleanup` in between. If a run
fails, the script stops the half-started VM; set `KEEP=1` to leave it running
for debugging.

The control-plane VM gets 2 vCPUs and 4 GiB of memory. Set `CP_VM_CPUS` to
change the vCPU count; if Cloud Hypervisor panics with passt vhost-user
networking and more than one vCPU (seen with v51), use `CP_VM_CPUS=1`.

## What gets cached

Everything lives under `/var/cache/qarax/hyperconverged/` when run as root
(including via `sudo`), otherwise under `~/.cache/qarax/hyperconverged/`
(`$XDG_CACHE_HOME`):

- The Cloud Hypervisor binary and `hypervisor-fw`.
- `base-<key>.raw`: the 8 GiB (sparse) root disk built from the control-plane
  image. It is rebuilt only when the image changes; the image itself rebuilds
  quickly when only the qarax binaries changed, since they are its last layers.
- `run/`: the current run's copy-on-write overlay of that disk, the token disk,
  sockets and logs (`console.log`, `cloud-hypervisor.log`, `passt.log`,
  `registry.log`).

The registry stores the pinned `grafana/otel-lgtm` image (and the `--with-db-vm`
Postgres image) in the podman volume `qarax-demo-registry`, so it is pulled
once. `--cleanup` keeps the cache and the volume; to reclaim the space, remove
the cache directory and run `podman volume rm qarax-demo-registry`.

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
| `Containerfile.control-plane` | OCI image for the control plane VM (qarax + qarax-node + PostgreSQL); build context is the repo root, pins come from `versions/` |
| `grafana/*.json` | Dashboards imported into the in-VM Grafana |
