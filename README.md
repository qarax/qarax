# qarax

## Description

qarax is a management platform for managing virtual machines with Cloud Hypervisor.

## Architecture

qarax consists of two main components:

- **qarax** (this repository): Control plane REST API server that manages VM and host lifecycle
- **qarax-node**: Data plane gRPC service that runs on hypervisor hosts and manages VM execution via Cloud Hypervisor

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
