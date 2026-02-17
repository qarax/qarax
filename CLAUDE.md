# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

qarax is a management platform for orchestrating virtual machines using Cloud Hypervisor. The system consists of two main components:

- **qarax** (control plane): REST API server built with Axum that manages VM and host lifecycle
- **qarax-node** (data plane): gRPC service that runs on hypervisor hosts and manages VM execution

## Architecture

### Workspace Structure

This is a Cargo workspace with the following crates:
- `qarax/`: Control plane - HTTP API server with database-backed state management
- `qarax-node/`: Data plane - gRPC service for VM operations on hypervisor hosts
- `common/`: Shared telemetry utilities using tracing
- `cloud-hypervisor-sdk`: Cloud Hypervisor Rust SDK (git dependency from github.com/qarax/cloud-hypervisor-sdk)

### qarax (Control Plane)

Main components:
- **handlers/**: Axum route handlers organized by resource (hosts, vms)
- **model/**: Data models and database queries (hosts, vms, storage)
- **database.rs**: Migration runner using sqlx
- **ansible.rs**: Ansible integration for host provisioning
- **configuration.rs**: Multi-environment config loading from `configuration/` directory
- **startup.rs**: Server initialization and routing setup

Key patterns:
- Uses Axum with tower middleware for tracing and request IDs
- PostgreSQL with sqlx for async database access and compile-time query verification
- Custom error types in `errors.rs` that map to HTTP status codes
- Environment-based configuration: Set `APP_ENVIRONMENT=production` or defaults to `local`
- Database migrations in `migrations/` directory run automatically on startup

### qarax-node (Data Plane)

Main components:
- **services/vm/**: VM lifecycle management using Cloud Hypervisor SDK
- **cloud_hypervisor/**: Manager for Cloud Hypervisor processes and API communication
- **rpc/**: gRPC service definitions generated from `proto/node.proto`

Key patterns:
- Uses tonic for gRPC server/client
- Uses the Cloud Hypervisor SDK for VM management via Unix socket API
- Spawns and manages Cloud Hypervisor processes directly (one CH instance per VM)
- CLI args parsing with clap
- Designed to run on hypervisor hosts and communicate with qarax control plane

### Database Schema

Three main tables with custom enum types:
- **hosts**: Hypervisor hosts with status tracking (DOWN, INSTALLING, UP, etc.)
- **vms**: VM definitions with vcpu, memory, kernel references
- **networks/network_interfaces**: Network configuration with MACVTAP support

## Development Commands

### Building

```bash
# Build all workspace members and generate OpenAPI spec
make build

# Or use cargo directly (won't generate OpenAPI)
cargo build

# Build release binaries
cargo build --release

# Build specific binary
cargo build -p qarax
cargo build -p qarax-node
```

Note: Project uses musl target (see `.cargo/config.toml`)

**Recommended**: Use `make build` instead of `cargo build` to ensure the OpenAPI spec is regenerated.

### Testing

```bash
# Run all unit tests
cargo test

# Run tests for specific package
cargo test -p qarax

# Run specific test
cargo test test_name

# Run test by name pattern
cargo test pattern

# Run E2E tests (requires Docker with KVM support)
cd e2e && ./run_e2e_tests.sh
```

### Run locally (full stack)

To run the full stack (qarax API + qarax-node + PostgreSQL) in Docker for local experimentation:

```bash
./hack/run_local.sh
```

Requirements: Docker (with Compose), KVM (`/dev/kvm`), Rust toolchain. The script builds the qarax-node binary, starts the E2E-style Compose stack, and prints API and Swagger URLs. Stop with `cd e2e && docker compose down -v`. Optional: `REBUILD=1` to rebuild images, `SKIP_BUILD=1` to skip building qarax-node.

### Code Quality

```bash
# Format code (requires nightly)
cargo +nightly fmt --all

# Check formatting without modifying
cargo +nightly fmt --all -- --check

# Run clippy linter
cargo clippy
```

### Database

Environment variables (see `.env.sample`):
- `DATABASE_URL`: PostgreSQL connection string for development
- `TEST_DATABASE_URL`: Separate database for tests
- `SQLX_OFFLINE=true`: Enable offline mode for sqlx compile-time verification

Migrations run automatically when qarax starts. The migrations are in `migrations/` and use sqlx's migration system.

### Running

```bash
# Run qarax control plane (requires PostgreSQL)
cargo run -p qarax

# Run qarax-node with custom port and paths
cargo run -p qarax-node -- --port 50051

# qarax-node CLI options:
#   --port <PORT>                    Port to listen on (default: 50051)
#   --runtime-dir <PATH>             Directory for VM sockets/logs (default: /var/lib/qarax/vms)
#   --cloud-hypervisor-binary <PATH> Path to cloud-hypervisor binary (default: /usr/local/bin/cloud-hypervisor)
```

qarax reads configuration from `configuration/base.yaml` merged with environment-specific files (`local.yaml` or `production.yaml`). Default server port is 8000.

## Protocol Buffers

The project uses Protocol Buffers for gRPC communication between qarax and qarax-node. Proto definitions are in `proto/node.proto` and compiled via `tonic-build` in build scripts. The `VmService` defines operations: StartVM, StopVM, ListVms, GetVmInfo.

## OpenAPI Documentation

The qarax control plane includes auto-generated OpenAPI 3.1 documentation using utoipa. The OpenAPI spec is version-controlled and should be regenerated whenever API endpoints change.

### Accessing the Documentation

When the qarax server is running:
- **Swagger UI**: http://localhost:8000/swagger-ui
- **OpenAPI JSON spec**: http://localhost:8000/api-docs/openapi.json
- **Committed YAML spec**: `qarax/openapi.yaml`

### Regenerating the OpenAPI Spec

The OpenAPI spec is automatically regenerated when you run `make build`. You can also generate it manually:

```bash
# Recommended: build and generate OpenAPI
make build

# Or generate OpenAPI only
make openapi

# Or use cargo directly
cargo run -p qarax --bin generate-openapi
```

This updates `qarax/openapi.yaml` which should be committed to the repository.

### Adding New Endpoints

When adding a new endpoint, follow these steps:

1. **Annotate the handler function** with `#[utoipa::path(...)]`:
   ```rust
   #[utoipa::path(
       get,
       path = "/resource/{id}",
       params(
           ("id" = uuid::Uuid, Path, description = "Resource ID")
       ),
       responses(
           (status = 200, description = "Success", body = Resource),
           (status = 404, description = "Not found")
       ),
       tag = "resources"
   )]
   pub async fn get_resource(...) -> Result<ApiResponse<Resource>> {
       // implementation
   }
   ```

2. **Add `ToSchema` derive** to any new data models:
   ```rust
   #[derive(Serialize, Deserialize, ToSchema)]
   pub struct Resource {
       pub id: Uuid,
       pub name: String,
   }
   ```

3. **Register in `handlers/mod.rs`** `ApiDoc`:
   - Add the handler path to the `paths(...)` list
   - Add any new schemas to the `components(schemas(...))` list

4. **Regenerate the spec**: Run `make build` or `make openapi`

5. **Commit the updated spec**: Include `qarax/openapi.yaml` in your commit

## Cloud Hypervisor SDK Integration

qarax-node uses the Cloud Hypervisor Rust SDK (from `github.com/qarax/cloud-hypervisor-sdk`) to communicate with Cloud Hypervisor instances via their Unix socket HTTP API.

### How It Works

1. **Process Management**: qarax-node spawns one `cloud-hypervisor` process per VM, each with its own API socket at `/var/lib/qarax/vms/{vm_id}.sock`

2. **SDK Usage**: The SDK provides:
   - Model types matching Cloud Hypervisor's API schema (`VmConfig`, `CpusConfig`, `MemoryConfig`, etc.)
   - `Machine::connect()` for connecting to existing CH instances
   - `VM` methods for boot, shutdown, and get_info
   - `TokioIo` adapter for Unix socket communication

3. **Raw API Calls**: For operations not in the SDK (pause, resume, device hotplug), the manager sends raw HTTP requests over Unix sockets

### Key Files

- `qarax-node/src/cloud_hypervisor/mod.rs` - Module exports
- `qarax-node/src/cloud_hypervisor/manager.rs` - VmManager that spawns CH processes and manages VM lifecycle
- `qarax-node/src/services/vm/mod.rs` - gRPC service implementation that delegates to VmManager

### Configuration Conversion

Proto types from `node.proto` are converted to SDK model types in `manager.rs`:
- `ProtoVmConfig` → `cloud_hypervisor_sdk::models::VmConfig`
- `ProtoCpusConfig` → `CpusConfig`
- `ProtoMemoryConfig` → `MemoryConfig`
- etc.

## E2E Testing

The `e2e/` directory contains end-to-end tests that spin up real VMs using Cloud Hypervisor.

### Test Infrastructure

- **docker-compose.yml**: Orchestrates qarax, qarax-node, and PostgreSQL containers
- **Dockerfile.qarax-node**: Builds the qarax-node container with Cloud Hypervisor and test boot artifacts
- **test_vm_lifecycle.py**: Python pytest tests for VM operations

### Test Boot Artifacts

The E2E tests use minimal boot artifacts for fast VM startup:

- **Kernel**: Linux 6.1.6 from Cloud Hypervisor test artifacts (`/var/lib/qarax/images/vmlinux`)
- **Initramfs**: Minimal BusyBox-based initramfs (`/var/lib/qarax/images/test-initramfs.gz`)
- **init-test.sh**: The init script that runs inside the VM - mounts filesystems, prints diagnostics, waits 5 seconds, then shuts down

### Running E2E Tests

```bash
cd e2e
./run_e2e_tests.sh
```

Requirements:
- Docker with KVM support (`/dev/kvm` passthrough)
- Pre-built release binaries (`cargo build --release`)

## Important Implementation Details

### Host Provisioning

Hosts are provisioned using bootc (bootable containers). VMM hosts boot from container images that include qarax-node, Cloud Hypervisor, and all required dependencies. This provides immutable infrastructure with atomic updates and rollback capability.

- **Development mode**: Direct binary deployment via SCP for fast iteration
- **Production mode**: bootc image deployment for consistency and version control

See `deployments/` directory for Containerfile and configuration.

### SQLX Offline Mode

The project uses `SQLX_OFFLINE=true` with checked-in query metadata in `sqlx-data.json`. When modifying queries, regenerate with:
```bash
cargo sqlx prepare --workspace
```

### Custom Postgres Types

Several custom ENUM types are defined in migrations and mapped to Rust enums using sqlx and strum:
- `host_status`: Host lifecycle states
- `vm_status`: VM states (UNKNOWN, CREATED, RUNNING, PAUSED, SHUTDOWN)
- `network_mode`: Static, DHCP, or None
- `hypervisor`: CLOUD_HV
- `interface_type`: MACVTAP, TAP, VHOST_USER
- `console_mode`: OFF, PTY, TTY, FILE, SOCKET, NULL

When working with these types, the Rust enums use derive macros for serialization and SQL mapping.

## Enhanced Cloud Hypervisor Support

qarax provides comprehensive support for Cloud Hypervisor's advanced VM configuration capabilities, aligning closely with the cloud-hypervisor-sdk models.

### CPU Configuration

VMs support detailed CPU configuration:
- **boot_vcpus**: Number of vCPUs available at boot
- **max_vcpus**: Maximum number of vCPUs (for future hotplug support)
- **CPU Topology**: Configure threads per core, cores per die, dies per package, and packages (stored as JSONB)
- **KVM Hyper-V**: Enable Hyper-V enlightenments for improved Windows guest performance

Example CPU topology:
```json
{
  "threads_per_core": 2,
  "cores_per_die": 4,
  "dies_per_package": 1,
  "packages": 1
}
```

### Memory Configuration

Memory is configured in bytes (BIGINT) for precision:
- **memory_size**: Base memory size in bytes
- **memory_hotplug_size**: Maximum hotplug memory size
- **memory_hugepages**: Enable hugepage support
- **memory_hugepage_size**: Hugepage size in bytes (e.g., 2MB, 1GB)
- **memory_shared**: Required for vhost-user devices
- **memory_mergeable**: Enable Kernel Samepage Merging (KSM)
- **memory_prefault**: Prefault memory on VM start
- **memory_thp**: Enable Transparent Huge Pages

### Advanced Networking (Priority Feature)

qarax supports multiple network interfaces per VM with advanced configuration:

**Multiple NICs**: Each VM can have multiple network interfaces, identified by unique `device_id` per VM.

**Interface Types**:
- **MACVTAP**: Traditional MACVTAP interface
- **TAP**: Pre-created TAP device
- **VHOST_USER**: High-performance userspace networking

**Network Configuration**:
- **tap_name**: Pre-created TAP device name
- **mac_address**: Guest MAC address
- **host_mac**: Host-side MAC address
- **ip_address**: IPv4 or IPv6 address
- **mtu**: Maximum Transmission Unit

**vhost-user Configuration**:
- **vhost_user**: Enable vhost-user mode
- **vhost_socket**: Unix socket path for vhost-user backend
- **vhost_mode**: CLIENT or SERVER mode

**Performance Tuning**:
- **num_queues**: Number of virtio queues (default: 1)
- **queue_size**: Size of each queue (default: 256)
- **rate_limiter**: Bandwidth and operations limiting (JSONB)

**Offload Features**:
- **offload_tso**: TCP Segmentation Offload
- **offload_ufo**: UDP Fragmentation Offload
- **offload_csum**: Checksum offload

**PCI Configuration**:
- **pci_segment**: PCI segment number
- **iommu**: Enable IOMMU for device

### Enhanced Disk Configuration

Disks support advanced Cloud Hypervisor features:

- **vhost-user**: High-performance userspace block devices
- **vhost_socket**: Unix socket path for vhost-user block backend
- **direct**: Enable O_DIRECT flag for direct I/O
- **num_queues**: Number of queues for multi-queue block devices (default: 1)
- **queue_size**: Queue size (default: 128)
- **rate_limiter**: Per-disk bandwidth and IOPS limiting (JSONB)
- **rate_limit_group**: Reference to shared rate limit group
- **pci_segment**: PCI segment number
- **serial_number**: Disk serial number

### VM Lifecycle States

VMs now have richer lifecycle states aligned with Cloud Hypervisor:

- **UNKNOWN**: Unknown state (error or legacy)
- **CREATED**: VM created but not started
- **RUNNING**: VM is actively running
- **PAUSED**: VM is paused
- **SHUTDOWN**: VM has shut down

### Device Support

**Console Devices**: Serial and console devices with multiple modes
- **OFF**: No console
- **PTY**: Pseudo-terminal
- **TTY**: Direct TTY
- **FILE**: Log to file
- **SOCKET**: Unix socket
- **NULL**: Discard output

**RNG Device**: Entropy source configuration (default: /dev/urandom)

### Rate Limiting

Rate limiting uses the token bucket algorithm with two independent limits:

**Bandwidth Limiting** (bytes/sec):
```json
{
  "bandwidth": {
    "size": 10485760,          // 10 MB bucket
    "refill_time": 1000,       // Refill every 1000ms
    "one_time_burst": 20971520 // 20 MB initial burst
  }
}
```

**Operations Limiting** (ops/sec):
```json
{
  "ops": {
    "size": 1000,      // 1000 operations bucket
    "refill_time": 1000 // Refill every 1000ms
  }
}
```

Rate limiters can be:
- **Inline**: Configured directly on network_interfaces or vm_disks (JSONB field)
- **Shared**: Referenced via rate_limit_groups table (multiple devices share one policy)

### Database Schema Updates

**Enhanced Tables**:
- **vms**: CPU topology, memory configuration in bytes, hotplug support
- **vm_disks**: vhost-user, direct I/O, rate limiting, queue configuration
- **network_interfaces**: Multiple NICs per VM, vhost-user, performance tuning, offload features

**New Tables**:
- **vm_consoles**: Serial and console device configuration per VM
- **vm_rng**: RNG device configuration per VM
- **rate_limit_groups**: Shared rate limiting policies

### Working with Multiple Network Interfaces

```sql
-- Add multiple NICs to a VM
INSERT INTO network_interfaces (vm_id, network_id, device_id, mac_address, ip_address, ...)
VALUES
  ('vm-uuid', 'net1-uuid', 'net0', '52:54:00:12:34:56', '192.168.1.10/24', ...),
  ('vm-uuid', 'net2-uuid', 'net1', '52:54:00:12:34:57', '10.0.0.5/24', ...);

-- Query NICs for a VM
SELECT * FROM network_interfaces WHERE vm_id = 'vm-uuid' ORDER BY device_id;
```

### vhost-user Configuration

For high-performance I/O, configure vhost-user:

**Network**:
```sql
UPDATE network_interfaces SET
  vhost_user = true,
  vhost_socket = '/var/run/vhost-user-net0.sock',
  vhost_mode = 'server'
WHERE device_id = 'net0';

-- Ensure VM has shared memory enabled
UPDATE vms SET memory_shared = true WHERE id = 'vm-uuid';
```

**Disk**:
```sql
UPDATE vm_disks SET
  vhost_user = true,
  vhost_socket = '/var/run/vhost-user-blk0.sock',
  storage_object_id = NULL  -- Not needed for vhost-user
WHERE disk_id = 'disk0';

-- Ensure VM has shared memory enabled
UPDATE vms SET memory_shared = true WHERE id = 'vm-uuid';
```

### Protobuf Schema

The protobuf schema (`proto/node.proto`) closely mirrors the Cloud Hypervisor SDK models:

- **VmConfig**: Root configuration with CPU, memory, payload, disks, networks, devices
- **CpusConfig**: boot_vcpus, max_vcpus, topology
- **MemoryConfig**: size, hotplug_size, hugepages, shared, mergeable
- **NetConfig**: Comprehensive network interface configuration
- **DiskConfig**: Disk configuration with vhost-user and rate limiting
- **PayloadConfig**: Kernel, cmdline, initramfs, firmware
- **ConsoleConfig**: Console device configuration
- **RngConfig**: RNG device configuration
- **RateLimiterConfig**: Token bucket rate limiting

### gRPC Service Methods

The VmService provides:
- **CreateVM**: Create VM with full configuration
- **StartVM**: Start a created VM
- **StopVM**: Stop a running VM
- **PauseVM**: Pause a running VM
- **ResumeVM**: Resume a paused VM
- **DeleteVM**: Delete a VM
- **GetVmInfo**: Get VM state and configuration
- **ListVms**: List all VMs
- **AddNetworkDevice**: Hot-attach network device
- **RemoveNetworkDevice**: Hot-detach network device
- **AddDiskDevice**: Hot-attach disk device
- **RemoveDiskDevice**: Hot-detach disk device
