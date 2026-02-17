# qarax

## Description

qarax is a management platform for managing virtual machines with Cloud Hypervisor.

## Architecture

qarax consists of two main components:

- **qarax** (this repository): Control plane REST API server that manages VM and host lifecycle
- **qarax-node**: Data plane gRPC service that runs on hypervisor hosts and manages VM execution via Cloud Hypervisor

## Building

```bash
# Build all packages and generate OpenAPI spec
make build

# Generate OpenAPI spec only
make openapi

# Run tests
make test

# Or use cargo directly (won't auto-generate OpenAPI)
cargo build
```

The project includes auto-generated OpenAPI 3.1 documentation. Access it at http://localhost:8000/swagger-ui when the server is running.

## Run locally (Docker stack)

To run the full stack (qarax + qarax-node + PostgreSQL) in Docker for local testing:

```bash
./hack/run_local.sh
```

Requires Docker, Docker Compose, KVM (`/dev/kvm`), and a Rust toolchain. The script builds qarax-node, starts all services, and prints the API and Swagger UI URLs. Stop with `cd e2e && docker compose down -v`.

## VM Boot Configuration

qarax uses configurable default boot artifacts for VMs. Configure these in your environment's YAML file:

```yaml
vm_defaults:
  kernel: "/var/lib/qarax/images/vmlinux"
  initramfs: "/var/lib/qarax/images/initramfs.gz"
  cmdline: "console=ttyS0 console=hvc0 root=/dev/vda1"
```

### Using Test Artifacts (E2E/Local Development)

For E2E tests and local development, the default configuration uses test artifacts that boot and shut down after 5 seconds. These are useful for verifying VM creation but not for running persistent VMs.

### Using Production Images

For production VMs that stay running, replace the test artifacts with proper bootable images:

1. **Option 1: Cloud Images** (Recommended)
   ```bash
   # Download a cloud image (e.g., Ubuntu)
   wget https://cloud-images.ubuntu.com/minimal/releases/jammy/release/ubuntu-22.04-minimal-cloudimg-amd64.img

   # Extract kernel and initrd from the image
   virt-get-kernel ubuntu-22.04-minimal-cloudimg-amd64.img

   # Update configuration to point to these files
   ```

2. **Option 2: Custom Build**
   Build your own kernel and initramfs with the tools and init system you need.

Update your `configuration/production.yaml`:
```yaml
vm_defaults:
  kernel: "/var/lib/qarax/images/vmlinux-production"
  initramfs: "/var/lib/qarax/images/initramfs-production.gz"
  cmdline: "console=ttyS0 console=hvc0 root=/dev/vda1 init=/sbin/init"
```

## Host Provisioning

qarax uses bootc (bootable containers) to deploy VMM (Virtual Machine Manager) hosts. The bootc image includes qarax-node, Cloud Hypervisor, and all necessary dependencies.

### Configuration

Add the deployment configuration to your configuration file (`configuration/base.yaml`):

```yaml
deployment:
  mode: "direct"  # or "bootc" for production
  ssh_key_path: "/path/to/ssh/key"
  
bootc:
  registry: "quay.io/yourorg"
  image_name: "qarax-vmm-host"
```

### Development Mode (Direct Deployment)

During development, use direct mode to quickly deploy qarax-node binaries:

```bash
# Build qarax-node
cargo build -p qarax-node

# Deploy to test host
scp target/debug/qarax-node root@192.168.1.100:/usr/local/bin/qarax-node
ssh root@192.168.1.100 "systemctl restart qarax-node"
```

### Production Mode (bootc Image Deployment)

For production, build and deploy bootc images:

```bash
# Build release binary
cargo build --release -p qarax-node

# Build bootc image
podman build -f deployments/Containerfile.qarax-vmm -t quay.io/yourorg/qarax-vmm-host:v1.0.0 .
podman push quay.io/yourorg/qarax-vmm-host:v1.0.0

# Deploy to host via qarax API
curl -X POST http://qarax:8000/hosts/{host_id}/deploy \
  -d '{"mode": "bootc", "image_version": "v1.0.0"}'
```

See [deployments/README.md](deployments/README.md) for detailed information about building and deploying bootc images.
