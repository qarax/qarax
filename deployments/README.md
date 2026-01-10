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

## Deploying to Hosts

### Initial Host Setup

Your target host needs to support bootc. Recommended base systems:

- Fedora CoreOS
- CentOS Stream with bootc installed
- RHEL 9.4+ with bootc

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

After reboot, the host will:
- Boot from the container image
- Load all kernel modules for VM support
- Apply networking configuration
- Start qarax-node automatically

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
  --build-arg CLOUD_HYPERVISOR_VERSION=v39.0 \
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

## Image Versioning Strategy

Recommended versioning scheme:

- `latest`: Latest stable build (not recommended for production)
- `v1.0.0`: Semantic versioning for releases
- `v1.0.0-rc1`: Release candidates
- `dev-YYYYMMDD-HASH`: Development builds

Example tagging:

```bash
# Build
podman build -f deployments/Containerfile.qarax-vmm -t qarax-vmm-host:build .

# Tag for release
podman tag qarax-vmm-host:build quay.io/yourorg/qarax-vmm-host:v1.2.3
podman tag qarax-vmm-host:build quay.io/yourorg/qarax-vmm-host:latest

# Push all tags
podman push quay.io/yourorg/qarax-vmm-host:v1.2.3
podman push quay.io/yourorg/qarax-vmm-host:latest
```

## Troubleshooting

### Image Build Fails

**Binary not found**:
```
Error: COPY target/release/qarax-node: file not found
```

Solution: Build the binary first:
```bash
cargo build --release -p qarax-node
```

**Architecture mismatch**:
If building on different architecture than target:
```bash
cargo build --release --target x86_64-unknown-linux-gnu -p qarax-node
```

### Host Won't Boot After Update

**Boot fails**:
- Host will automatically rollback to previous working image
- Check serial console or IPMI for boot errors
- Verify image was built correctly

**Manual rollback**:
```bash
# From rescue mode or previous boot
bootc rollback
reboot
```

### qarax-node Won't Start

**Check logs**:
```bash
ssh root@vmm-host "journalctl -u qarax-node -n 50"
```

**Common issues**:
- Port 50051 already in use
- Missing dependencies (check Containerfile)
- Incorrect permissions on /var/lib/qarax

**Test binary manually**:
```bash
ssh root@vmm-host "/usr/local/bin/qarax-node --help"
```

### Networking Issues

**VMs can't communicate**:
- Check kernel modules loaded: `lsmod | grep kvm`
- Verify sysctl settings: `sysctl net.ipv4.ip_forward`
- Check firewall: `firewall-cmd --list-all`

**Apply config manually**:
```bash
ssh root@vmm-host "sysctl -p /etc/sysctl.d/99-vm-network.conf"
```

## Security Considerations

### Image Registry Access

- Use private registries for production images
- Configure registry authentication on hosts
- Use image signing and verification (cosign)

### Host Security

The image includes:
- Minimal software surface
- systemd hardening (NoNewPrivileges, PrivateTmp)
- Firewall configuration
- Security-focused sysctl settings

### Updates

- Regularly update base image (FROM layer)
- Pin versions of critical components
- Test updates in staging before production
- Monitor security advisories for dependencies

## Further Reading

- [bootc documentation](https://github.com/containers/bootc)
- [Cloud Hypervisor](https://github.com/cloud-hypervisor/cloud-hypervisor)
- [Container Best Practices](https://docs.docker.com/develop/dev-best-practices/)
