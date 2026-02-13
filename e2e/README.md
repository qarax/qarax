# Qarax E2E Tests

End-to-end tests for the Qarax VM management platform using Docker Compose and the Python SDK.

## Overview

These tests verify the complete VM lifecycle by running:
- **qarax** (control plane) - REST API server
- **qarax-node** (data plane) - gRPC service with Cloud Hypervisor
- **postgres** - Database

All services run in containers with KVM passthrough for real VM operations.

## What Gets Tested

The E2E tests create **real virtual machines** using Cloud Hypervisor:

1. **qarax-node** spawns actual Cloud Hypervisor processes
2. Each VM boots a minimal test kernel with initramfs
3. Full lifecycle is verified: create → boot → pause → resume → shutdown → delete
4. Multiple VMs can run concurrently

The test environment includes:
- **Test kernel**: Linux 6.1.6 built for Cloud Hypervisor
- **Test initramfs**: Minimal BusyBox-based initramfs that boots and halts
- **Cloud Hypervisor v44.0**: VMM for running microVMs

## Prerequisites

- **Docker/Podman Compose**: For running services
- **KVM support**: `/dev/kvm` must be accessible
- **uv**: Fast Python package manager
  ```bash
  curl -LsSf https://astral.sh/uv/install.sh | sh
  ```
- **Rust toolchain**: For building qarax binaries

Check KVM access:
```bash
ls -la /dev/kvm
# Should show read/write access for your user
```

## Quick Start

```bash
cd e2e
./run_e2e_tests.sh
```

This will:
1. Build qarax-node binary (if needed)
2. Start all services via docker-compose
3. Run the E2E tests
4. Clean up

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `REBUILD` | Force rebuild of all images | - |
| `KEEP` | Keep services running after tests | - |
| `SKIP_BUILD` | Skip building qarax-node binary | - |

Examples:
```bash
# Keep services running for debugging
KEEP=1 ./run_e2e_tests.sh

# Force rebuild everything
REBUILD=1 ./run_e2e_tests.sh

# Skip binary build (use existing)
SKIP_BUILD=1 ./run_e2e_tests.sh
```

## Debugging

When tests fail, keep services running:
```bash
KEEP=1 ./run_e2e_tests.sh
```

Then debug:
```bash
# View logs
docker-compose logs -f
docker-compose logs qarax-node

# Shell into qarax-node
docker-compose exec qarax-node sh

# Check qarax API
curl http://localhost:8000/vms

# Stop when done
docker-compose down -v
```

## Running Individual Tests

```bash
# Install deps once
uv sync

# Run all tests
uv run pytest test_vm_lifecycle.py -v

# Run specific test
uv run pytest test_vm_lifecycle.py::test_vm_create_and_list -v
uv run pytest test_vm_lifecycle.py::test_vm_full_lifecycle -v
```

## Architecture

```
Docker Compose
├── qarax (control plane) :8000
├── qarax-node (data plane) :50051
│   └── Cloud Hypervisor (with /dev/kvm)
└── postgres :5432

E2E Tests (Python + pytest)
└── qarax-api-client SDK -> qarax :8000
```

## Test Cases

| Test | Description |
|------|-------------|
| `test_vm_create_and_list` | Create VM, verify in list |
| `test_vm_full_lifecycle` | Create → Start → Pause → Resume → Stop → Delete |
| `test_vm_delete` | Create and delete VM |
| `test_multiple_vms` | Create/manage multiple VMs |
| `test_vm_start_stop_cycle` | Start/stop VM multiple times |

## Troubleshooting

### KVM Permission Denied

```bash
# Add user to kvm group
sudo usermod -aG kvm $USER
newgrp kvm
```

### Service Won't Start

```bash
# Check logs
docker-compose logs qarax-node
docker-compose logs qarax

# Rebuild images
REBUILD=1 ./run_e2e_tests.sh
```

### Tests Fail with Connection Errors

```bash
# Ensure services are healthy
docker-compose ps

# Check connectivity
curl http://localhost:8000/
nc -zv localhost 50051
```
