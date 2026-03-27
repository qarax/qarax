# Hyperconverged Demo

Run the qarax control plane (API + PostgreSQL) inside a Cloud Hypervisor VM on bare metal, with the same VM also running `qarax-node` — the "hosted engine" pattern.

```
Host (bare metal)
├── TAP: qarax-cp-tap0 (192.168.100.1/24)
├── Local OCI registry (port 5000)
└── Cloud Hypervisor VM: control-plane
    ├── eth0 (192.168.100.10/24)
    ├── qarax API (port 8000)
    ├── qarax-node (port 50051)
    ├── overlaybd-tcmu
    └── PostgreSQL (local)
```

## Prerequisites

- Linux host with KVM and nested KVM (`kvm_intel.nested=Y`)
- Rust toolchain
- `podman`
- `cloud-hypervisor` on PATH (auto-downloaded if missing)
- Root / sudo access

## Usage

```bash
# Full build + run (defaults to passt-backed workload VM networking)
sudo ./demos/hyperconverged/run.sh

# Skip cargo build (use existing binaries)
sudo SKIP_BUILD=1 ./demos/hyperconverged/run.sh

# With optional extras
sudo ./demos/hyperconverged/run.sh --with-local         # also create a local storage pool
sudo ./demos/hyperconverged/run.sh --with-nfs --nfs-url server:/export
sudo ./demos/hyperconverged/run.sh --with-local-vm      # boot a firmware VM with cloud image
sudo ./demos/hyperconverged/run.sh --with-db-vm         # boot an OCI PostgreSQL VM
sudo ./demos/hyperconverged/run.sh --network-backend bridge

# Tear down
sudo ./demos/hyperconverged/run.sh --cleanup
```

## After startup

```bash
export QARAX_SERVER=http://192.168.100.10:8000
qarax vm list
qarax vm attach alpine-vm
```

`passt` is the default backend for workload VMs in this demo because it avoids
per-network bridge, DHCP, and NAT setup inside the nested node. Use
`--network-backend bridge` if you specifically want bridged Qarax-managed guest
networking instead.

## Files

| File | Description |
|------|-------------|
| `run.sh` | Demo orchestration script |
| `Containerfile.control-plane` | OCI image for the control plane VM (qarax + qarax-node + PostgreSQL) |
