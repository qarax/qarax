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
- `vm_status`: VM states (UP, DOWN, UNKNOWN)
- `network_mode`: Static, DHCP, or None
- `hypervisor`: CLOUD_HV

When working with these types, the Rust enums use derive macros for serialization and SQL mapping.
