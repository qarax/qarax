# Qarax VMM Host Deployments

This directory contains everything needed to build and deploy bootc images for qarax VMM (Virtual Machine Manager) hosts.

## Overview

qarax uses **bootc** (bootable containers) to deploy VMM hosts. This provides:

- **Immutable infrastructure**: Hosts boot from container images
- **Atomic updates**: Switch between image versions with rollback capability
- **Consistency**: All hosts run identical, versioned images
- **Fast deployment**: Pull image and reboot instead of configuration management

## Files in This Directory

- `Containerfile.qarax-vmm`: Defines the bootc image for VMM hosts
- `qarax-node.service`: systemd unit for running qarax-node
- `vm-network.conf`: sysctl configuration for VM networking
- `vm-modules.conf`: Kernel modules to load for VM support
- `README.md`: This file

## Building the Image

### Prerequisites

1. **Build qarax-node binary**:
   ```bash
   cargo build --release -p qarax-node
   ```

2. **Install podman or docker**:
   ```bash
   # Fedora/RHEL/CentOS
   sudo dnf install podman

   # Ubuntu/Debian
   sudo apt install podman
   ```

### Build the Image

```bash
# From the qarax repository root:
podman build \
  -f deployments/Containerfile.qarax-vmm \
  --build-arg CLOUD_HYPERVISOR_VERSION="$(cat versions/cloud-hypervisor-version)" \
  --build-arg FIRECRACKER_VERSION="$(cat versions/firecracker-version)" \
  -t quay.io/yourorg/qarax-vmm-host:v1.0.0 \
  .
```

### Push to Registry

```bash
# Login to your container registry
podman login quay.io

# Push the image
podman push quay.io/yourorg/qarax-vmm-host:v1.0.0

# Tag as latest
podman tag quay.io/yourorg/qarax-vmm-host:v1.0.0 quay.io/yourorg/qarax-vmm-host:latest
podman push quay.io/yourorg/qarax-vmm-host:latest
```

### Publish via GitHub Actions (Recommended)

This repository includes `.github/workflows/appliance-release.yml` to publish a bootc appliance image to GHCR:

- Image repository: `ghcr.io/<org>/qarax-vmm-host`
- Trigger automatically on pushes to `master`
- Trigger on git tags matching `v*` for versioned releases
- Or run manually with `workflow_dispatch` to publish ad-hoc/dev tags

On `master` pushes, the workflow publishes:

- `ghcr.io/<org>/qarax-vmm-host:master-<short-sha>`
- `ghcr.io/<org>/qarax-vmm-host:sha-<git-sha>`
- `ghcr.io/<org>/qarax-vmm-host:latest`

Versioned release example:

```bash
git tag v1.0.0
git push origin v1.0.0
```

For release tags, the workflow publishes:
- `ghcr.io/<org>/qarax-vmm-host:v1.0.0`
- `ghcr.io/<org>/qarax-vmm-host:sha-<git-sha>`
- `ghcr.io/<org>/qarax-vmm-host:latest` (for tag-triggered releases)

It also signs the published digest with cosign keyless signing.

## Deploying to Hosts

### Initial Host Setup

Your target host needs to support bootc. Recommended base systems:

- Fedora CoreOS
- CentOS Stream with bootc installed

If starting from a standard system:

```bash
# On the target host:
sudo dnf install bootc

# Enable bootc
sudo bootc install to-existing-root
```

### Deploy the Image

```bash
# SSH to the target host
ssh root@vmm-host

# Switch to the qarax VMM image
bootc switch quay.io/yourorg/qarax-vmm-host:v1.0.0

# Reboot into the new image
systemctl reboot
```

### Deploy via Qarax API (Bootc)

You can trigger the same flow through qarax instead of running SSH commands manually.

```bash
# 1) Register the host in qarax (port is qarax-node gRPC port)
curl -s -X POST http://localhost:8000/hosts \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "vmm-host-1",
    "address": "10.0.0.42",
    "port": 50051,
    "host_user": "root",
    "password": ""
  }'

# 2) Start bootc deployment over SSH
curl -s -X POST http://localhost:8000/hosts/<host-id>/deploy \
  -H 'Content-Type: application/json' \
  -d '{
    "image": "quay.io/yourorg/qarax-vmm-host:v1.0.0",
    "ssh_port": 22,
    "ssh_user": "root",
    "ssh_private_key_path": "/home/qarax/.ssh/id_ed25519",
    "install_bootc": true,
    "reboot": true
  }'
```

Deployment runs asynchronously. Track progress with:

```bash
curl -s http://localhost:8000/hosts | jq
```

Host status transitions:
- `down` -> `installing` when deployment starts
- `up` on success
- `installation_failed` on deployment error

After reboot, the host will:
- Boot from the container image
- Load all kernel modules for VM support
- Apply networking configuration
- Start qarax-node automatically
- Have both `cloud-hypervisor` and `firecracker` installed under `/usr/local/bin/`

### Verify Deployment

```bash
# Check qarax-node is running
ssh root@vmm-host "systemctl status qarax-node"

# Check which image is active
ssh root@vmm-host "bootc status"

# Test connectivity
ssh root@vmm-host "/usr/local/bin/qarax-node --version"
```

## Development Workflow

During development, you don't need bootc images. Use **direct deployment**:

### Quick Deploy for Development

```bash
# Build debug binary
cargo build -p qarax-node

# Copy to test host
scp target/debug/qarax-node root@test-host:/usr/local/bin/qarax-node

# Restart service
ssh root@test-host "systemctl restart qarax-node"
```

### Watch Mode for Active Development

```bash
# Terminal 1: Auto-rebuild and deploy on changes
cargo watch -x 'build -p qarax-node' -s 'scp target/debug/qarax-node root@test-host:/usr/local/bin/ && ssh root@test-host systemctl restart qarax-node'

# Terminal 2: Watch logs
ssh root@test-host "journalctl -u qarax-node -f"
```

## Updating Hosts

### Update to New Version

```bash
# On the host or via qarax API:
ssh root@vmm-host "bootc switch quay.io/yourorg/qarax-vmm-host:v1.1.0 && systemctl reboot"
```

The host will:
1. Download the new image
2. Prepare the new bootloader entry
3. Reboot into the new version
4. Keep the old version available for rollback

### Rollback to Previous Version

```bash
# If something goes wrong with the new version:
ssh root@vmm-host "bootc rollback && systemctl reboot"
```

### Check Available Images

```bash
ssh root@vmm-host "bootc status"
```

Output shows:
- Current booted image
- Available images
- Rollback targets

## Customizing the Image

### Add Additional Software

Edit `Containerfile.qarax-vmm`:

```dockerfile
# Add your packages
RUN dnf install -y \
    your-package \
    another-tool
```

### Change Cloud Hypervisor Version

```bash
podman build \
  --build-arg CLOUD_HYPERVISOR_VERSION="$(cat versions/cloud-hypervisor-version)" \
  -f deployments/Containerfile.qarax-vmm \
  -t quay.io/yourorg/qarax-vmm-host:v1.1.0 \
  .
```

### Modify qarax-node Configuration

Add a configuration file:

```dockerfile
# In Containerfile
COPY deployments/qarax-node-config.yaml /etc/qarax-node/config.yaml
```

Update the service file to use it:

```ini
# In qarax-node.service
ExecStart=/usr/local/bin/qarax-node --port 50051 --config /etc/qarax-node/config.yaml
```
