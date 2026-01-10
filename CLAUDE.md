# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

qarax is a management platform for orchestrating virtual machines using Cloud Hypervisor. The system consists of two main components:

- **qarax** (control plane): REST API server built with Axum that manages VM and host lifecycle
- **qarax-node** (data plane): gRPC service that runs on hypervisor hosts and manages VM execution

## Architecture

### Workspace Structure

This is a Cargo workspace with three crates:
- `qarax/`: Control plane - HTTP API server with database-backed state management
- `qarax-node/`: Data plane - gRPC service for VM operations on hypervisor hosts
- `common/`: Shared telemetry utilities using tracing

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
- **services/vm/**: VM lifecycle management
- **rpc/**: gRPC service definitions generated from `proto/node.proto`

Key patterns:
- Uses tonic for gRPC server/client
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
# Build all workspace members
cargo build

# Build release binaries
cargo build --release

# Build specific binary
cargo build -p qarax
cargo build -p qarax-node
```

Note: Project uses musl target (see `.cargo/config.toml`)

### Testing

```bash
# Run all tests
cargo test

# Run tests for specific package
cargo test -p qarax

# Run specific test
cargo test test_name

# Run test by name pattern
cargo test pattern
```

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

# Run qarax-node with custom port
cargo run -p qarax-node -- --port 50051
```

qarax reads configuration from `configuration/base.yaml` merged with environment-specific files (`local.yaml` or `production.yaml`). Default server port is 8000.

## Protocol Buffers

The project uses Protocol Buffers for gRPC communication between qarax and qarax-node. Proto definitions are in `proto/node.proto` and compiled via `tonic-build` in build scripts. The `VmService` defines operations: StartVM, StopVM, ListVms, GetVmInfo.

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
