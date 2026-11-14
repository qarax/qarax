# qarax-node

Data plane gRPC service for the qarax VM management platform. Runs on hypervisor hosts and manages VM lifecycle via Cloud Hypervisor.

## What it does

- Manages Cloud Hypervisor processes (create, start, stop, pause, resume, delete, snapshot, migrate)
- Handles OverlayBD lazy-pull block devices for OCI images
- Creates and manages Linux bridges, TAP devices, in-process DHCP, and NAT rules
- Serves file transfers (HTTP download and local copy) into storage pools
- Reports host info (CPU, memory, disk, GPUs, NUMA topology) to the control plane
- Recovers running VMs on restart by reconnecting to surviving Cloud Hypervisor sockets

## Building

```bash
cargo build -p qarax-node --release --target x86_64-unknown-linux-musl
```

## Configuration

All flags can also be set as env vars.

| Flag | Default | Description |
|------|---------|-------------|
| `-p, --port` | `50051` | gRPC listen port |
| `--runtime-dir` | `/var/lib/qarax/vms` | VM sockets, logs, persisted config |
| `--cloud-hypervisor-binary` | `/usr/local/bin/cloud-hypervisor` | Path to CH binary |
| `--qarax-init-binary` | `/usr/local/bin/qarax-init` | PID 1 init for OCI VMs (disabled if absent) |
| `--convertor-binary` | `/opt/overlaybd/snapshotter/convertor` | OverlayBD converter (disabled if absent) |
| `--overlaybd-cache-dir` | `/var/lib/qarax/overlaybd` | OverlayBD per-VM configs and upper layers |

Env vars: `RUST_LOG` (tracing filter), `INSECURE_REGISTRIES` (comma-separated hosts for HTTP registry access).

## Runtime dependencies

| Dependency | Required | Purpose |
|---|---|---|
| `cloud-hypervisor` | Yes | VMM binary (one process per VM) |
| `/dev/kvm` | Yes | Hardware virtualization |
| `qarax-init` | No | PID 1 init injected into OCI VMs |
| `overlaybd-tcmu` | No | TCMU daemon for lazy block-level image loading |
| `convertor` | No | OCI to OverlayBD format conversion |
| `iptables`, `nft`, `ip` | For networking | NAT, VPC isolation and VM firewall rules; TAP and VXLAN devices |

Kernel modules: `kvm`, `kvm_intel` (or `kvm_amd`), `vhost_net`, `tap`, `tun`. For OverlayBD: `target_core_user`, `tcm_loop`.

## Storage backends

Three pool types, each implementing attach/detach/map/unmap:

- **local** -- Creates a directory at the configured path or `/var/lib/qarax/pools/{pool_id}`. Files are served directly.
- **nfs** -- Mounts an NFS share at `/var/lib/qarax/pools/{pool_id}`.
- **overlaybd** -- Connects to an OCI registry. Maps images as block devices via TCMU backstores with writable upper layers.

## Networking

- Creates Linux bridges via rtnetlink (isolated or bridged to a physical NIC)
- TAP devices are created per VM NIC and attached to the bridge, also for NICs
  hotplugged into a running VM. A NIC without a configured MAC gets one derived
  from the VM and device IDs.
- Runs an in-process DHCP server per bridge that answers only this node's VM
  NICs, each with its IPAM address and the network's mask; other clients (on a
  LAN the bridge is attached to) get no reply
- Runs a DNS forwarder on each isolated bridge's gateway address, relaying to
  the resolvers in the host's `/etc/resolv.conf`
- Sets up NAT via iptables MASQUERADE + FORWARD rules
- Filters every VM NIC at its bridge port, in the nftables table `bridge
  qarax`: the NIC may only send from its MAC and address, and the VM's
  security groups apply to all of its traffic, including traffic from VMs on
  the same bridge. Traffic to and from the host itself (DHCP, DNS, the node's
  own connections) isn't filtered by security groups.
- Sizes VPC overlay (VXLAN) devices from the underlay MTU, and accepts VXLAN
  traffic only from overlay peers (table `inet qarax_vxlan`)

## Deployment

qarax-node is packaged as a bootc container image (see `deployments/Containerfile.qarax-vmm`). The image includes Cloud Hypervisor, OverlayBD, and all dependencies. Deploy to a host with:

```bash
make appliance-build
make appliance-push
qarax host deploy node-01 --image ghcr.io/yourorg/qarax-vmm-host:latest
```

Or run it directly from the Docker Compose stack via `make run-local`.
