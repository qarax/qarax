# Qarax Local Testing Scripts

Quick reference for running qarax locally with VMs.

## Quick Start

### 1. Start the Stack

```bash
# Start the stack only
./hack/run_local.sh

# Start the stack and create an example VM with SSH access
./hack/run_local.sh --with-vm
```

### 2. Create a Test VM

```bash
# Create a VM with networking
./hack/create-test-vm.sh my-vm-name

# Or create without a name (auto-generates name)
./hack/create-test-vm.sh
```

### 3. Cleanup

```bash
./hack/run_local.sh --cleanup
```

## Key Differences: Default vs --with-vm

### Default (no flags)
- ✅ Fast startup
- ✅ Minimal resources
- ✅ Good for API testing
- No VM is created automatically

### With --with-vm
- ✅ Persistent VMs with SSH access
- ✅ Full Alpine Linux rootfs
- ⚠️ Slower first run (downloads and builds rootfs)
- Uses locally built kernel, initramfs, and rootfs in `e2e/local-test-images/`

## Network Configuration

All VMs created by the scripts now automatically:
1. Create a TAP device (e.g., `tap0`, `tap-0104`)
2. Attach it to the VM's network interface
3. Configure the network in Cloud Hypervisor

You can verify with:
```bash
# Check TAP devices on the node
docker compose -f e2e/docker-compose.yml exec qarax-node ip link show | grep tap

# Check VM network config
VM_ID=<your-vm-id>
docker compose -f e2e/docker-compose.yml exec qarax-node \
  curl -s --unix-socket /var/lib/qarax/vms/${VM_ID}.sock \
  http://localhost/api/v1/vm.info | jq '.config.net'
```

## Common Issues

### No network interface in VM (eth0 missing)
**Cause**: VM created without a TAP device
**Solution**: Use the updated scripts which auto-create TAP devices

### Empty rootfs with --with-vm
**Cause**: Rootfs creation failed
**Solution**: Run `./hack/run_local.sh --cleanup` and retry

### TAP device creation fails
**Cause**: `/dev/net/tun` not available
**Solution**:
```bash
sudo modprobe tun
sudo mkdir -p /dev/net
sudo mknod /dev/net/tun c 10 200
sudo chmod 0666 /dev/net/tun
```

## Useful Commands

### View VM console
```bash
VM_ID=<your-vm-id>
docker compose -f e2e/docker-compose.yml exec qarax-node \
  tail -f /var/lib/qarax/vms/${VM_ID}.console.log
```

### List VMs
```bash
curl -s http://localhost:8000/vms | jq
```

### Stop a VM
```bash
curl -X POST http://localhost:8000/vms/<vm-id>/stop
```

### Delete a VM
```bash
curl -X DELETE http://localhost:8000/vms/<vm-id>
```

### Check qarax-node logs
```bash
docker compose -f e2e/docker-compose.yml logs -f qarax-node
```

### Check qarax control plane logs
```bash
docker compose -f e2e/docker-compose.yml logs -f qarax
```

## API Documentation

Once the stack is running:
- **Swagger UI**: http://localhost:8000/swagger-ui
- **OpenAPI JSON**: http://localhost:8000/api-docs/openapi.json
- **API root**: http://localhost:8000/

## Environment Variables

- `REBUILD=1`: Force rebuild Docker images
- `SKIP_BUILD=1`: Skip building qarax-node binary
- `QARAX_API`: Override API URL (default: http://localhost:8000)

## Script Details

### run_local.sh
Main script that:
- Builds Docker images
- Starts postgres, qarax, qarax-node
- Registers the host
- Optionally creates and starts a VM

Flags:
- `--with-vm`: Create and start an example VM (builds Alpine rootfs with SSH on first run)
- `--cleanup`: Stop and remove everything

### create-test-vm.sh
Simple standalone script to create a VM with networking. Useful for:
- Testing VM creation
- Creating multiple VMs
- Understanding the VM creation flow

## Troubleshooting

### Services won't start
```bash
# Check Docker is running
docker ps

# Check logs
docker compose -f e2e/docker-compose.yml logs

# Rebuild from scratch
./hack/run_local.sh --cleanup
REBUILD=1 ./hack/run_local.sh
```

### VM won't start
```bash
# Check qarax-node logs
docker compose -f e2e/docker-compose.yml logs qarax-node

# Verify host is registered and up
curl -s http://localhost:8000/hosts | jq
```

### Can't connect to qarax-node
```bash
# From qarax container
docker compose -f e2e/docker-compose.yml exec qarax nc -zv qarax-node 50051
```
