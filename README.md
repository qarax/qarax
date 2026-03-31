# qarax

qarax is a management platform for virtual machines running on Cloud Hypervisor.

## Architecture

qarax consists of two main components:

- **[qarax](qarax/)** (control plane) -- Axum REST API server managing VM and host lifecycle, backed by PostgreSQL
- **[qarax-node](qarax-node/)** (data plane) -- gRPC service running on hypervisor hosts, managing VM execution via Cloud Hypervisor

Supporting crates:

- **[cli](cli/)** -- Command-line client for the qarax API
- **[qarax-init](qarax-init/)** -- Minimal PID 1 init process for OCI-booted VMs
- **[common](common/)** -- Shared logging and telemetry helpers

Communication flow: control plane -> gRPC (`proto/node.proto`) -> qarax-node -> Cloud Hypervisor API.

## Building

```bash
make build       # build all packages and generate OpenAPI spec
make test        # run tests (auto-starts Postgres via Docker)
make lint        # cargo clippy --workspace -- -D warnings
```

## Running locally

```bash
make run-local           # start full Docker stack (qarax + qarax-node + Postgres)
make run-local VM=1      # same, but boot a test VM with SSH access
make stop-local          # stop the stack
```

Requires Docker, Docker Compose, KVM (`/dev/kvm`), and a Rust toolchain.

The API serves Swagger UI at `http://localhost:8000/swagger-ui`.

## CLI quickstart

See the [CLI README](cli/) for full usage. Quick version:

```bash
cargo build -p cli --release

qarax configure --server http://localhost:8000
qarax host list
qarax vm list
```

## Host provisioning

qarax uses bootc (bootable containers) to deploy hypervisor hosts. The appliance image includes qarax-node, Cloud Hypervisor, and all dependencies.

```bash
# Register the host
qarax host add --name node-01 --address 10.0.0.42 --user root

# Build and push the appliance
make appliance-build
make appliance-push

# Deploy and initialize
qarax host deploy node-01 --image ghcr.io/yourorg/qarax-vmm-host:latest --ssh-key ~/.ssh/id_ed25519
qarax host init node-01
```

See the [qarax-node README](qarax-node/) for runtime dependencies and configuration.

## VM boot configuration

Default boot artifacts are configured per environment in `configuration/` (`base.yaml`, `local.yaml`, `production.yaml`), selected by the `APP_ENVIRONMENT` env var (default: `local`):

```yaml
vm_defaults:
  kernel: "/var/lib/qarax/images/vmlinux"
  initramfs: "/var/lib/qarax/images/initramfs.gz"
  cmdline: "console=ttyS0 console=hvc0 root=/dev/vda1"
```

## Demos

Working demo setups in `demos/`:

| Demo | Description |
|------|-------------|
| `oci/` | OverlayBD lazy-pull disk workflow |
| `boot-source/` | Direct kernel + initramfs boot |
| `hooks/` | Lifecycle webhooks |
| `etcd-cluster/` | 3-node etcd cluster on VMs |
| `k8s-cluster/` | 3-node kubeadm Kubernetes cluster |
| `gpu-passthrough/` | VFIO GPU passthrough |
| `hyperconverged/` | Single-VM control plane + node |
| `sandbox/` | Ephemeral auto-reaping VMs |
| `sse-events/` | Server-Sent Events stream |
