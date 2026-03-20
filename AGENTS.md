# AGENTS.md

This file provides guidance to AI coding agents working with this repository.

## Project Overview

Qarax is a two-tier VM management platform for Cloud Hypervisor:
- **qarax** (control plane): Axum REST API managing VM/host lifecycle, backed by PostgreSQL
- **qarax-node** (data plane): gRPC service (tonic) running on hypervisor hosts, managing VMs via Cloud Hypervisor SDK
- **common**: Shared logging/telemetry helpers
- **cli**: Command-line client (clap)
- **qarax-init**: PID 1 init process for OCI-booted VMs

Communication flow: Control plane → gRPC (`proto/node.proto`) → Data plane → Cloud Hypervisor API. VMs are scheduled onto hosts in "up" state; subsequent VM operations route to the scheduled host.

## Build & Development Commands

```bash
make build          # Build all + regenerate OpenAPI spec
make test           # Run tests (auto-starts Postgres via Docker)
make test-deps      # Start Postgres in Docker for tests
make lint           # cargo clippy --workspace -- -D warnings
make fmt            # cargo fmt + shfmt
make openapi        # Regenerate openapi.yaml
make sdk            # Regenerate Python SDK from OpenAPI
make run-local      # Start full Docker stack (qarax + qarax-node + Postgres)
make run-local VM=1 # Same, but run qarax-node in a libvirt VM
make stop-local      # Stop and remove the local Docker stack
make appliance-build # Build bootc container image for qarax-node (hypervisor host)
make appliance-push  # Push appliance image to registry
make ruff-check      # Lint the Python SDK (run after make sdk)
SKIP_DOCKER=1 make test  # Run tests skipping Postgres startup (if already running)
```

To verify changes, run: `make fmt lint sdk build openapi test`.

Build/test a single crate: `cargo build -p qarax-node` or `cargo nextest run -p qarax -E 'test(test_name)'`

**macOS note:** The default build target is `x86_64-unknown-linux-musl` (in `.cargo/config.toml`), but the Makefile auto-detects macOS and overrides to the host target. To build manually on macOS, pass `--target` explicitly or use `make build`.

**CI uses nightly for fmt:** `cargo +nightly fmt --all -- --check`

**Build dependencies:** `protobuf-compiler` (protoc) is required for gRPC code generation. `shfmt` is required by `make fmt` for shell script formatting.

## Database

PostgreSQL 16+ with SQLx (compile-time verified queries).

- Migrations in `migrations/`, run automatically on startup
- Start DB: `make test-deps` or `./scripts/start_db.sh`
- Default connection: `qarax:qarax@127.0.0.1:5432/qarax`
- After modifying any SQL query: `cargo sqlx prepare --workspace`
- Offline/CI builds: `SQLX_OFFLINE=true cargo build`

## Deployment Topology

Qarax supports two deployment modes — understand which one is in use before debugging:

- **Docker Compose (e2e / local dev):** `qarax` (control plane), `qarax-node`, and `postgres` run as separate containers. `qarax-node` runs privileged with `/dev/kvm` passthrough for real VMs.
- **Hyperconverged (libvirt demo):** A single Cloud Hypervisor VM hosts both the control plane and the node. `qarax-node` runs *inside* the VM alongside `qarax`. The VM itself runs on the developer's host via libvirt. `hack/run-local.sh --vm` and `hack/test-host-deploy-libvirt.sh` manage this mode.

Always confirm the deployment mode before debugging. Many issues (networking, storage mount paths, device availability) differ between the two modes.

## Debugging Protocol

Follow this order before touching code:

1. **Confirm the deployment mode** (container vs hyperconverged VM — see above).
2. **Check that required services are running** before looking at code:
   - Docker Compose mode: `docker compose ps` and `docker compose logs qarax-node`
   - Hyperconverged mode: `systemctl status qarax-node`, `ps aux | grep cloud-hypervisor`
   - Verify Postgres: `docker compose ps postgres` or `pg_isready -h 127.0.0.1 -p 5432`
3. **Read service logs** before grep-ing source files. Most runtime failures are infrastructure, not code.
4. **Only after confirming infrastructure is healthy**, investigate code-level issues.

Do not jump to code investigation when services may not be running or the wrong environment is assumed.

## Architecture

### Control Plane (qarax)

- `qarax/src/handlers/` — Axum HTTP handlers by resource (hosts, vms, storage_pools, storage_objects, boot_sources, transfers, jobs)
- `qarax/src/model/` — Database models with inline SQLx queries
- `qarax/src/grpc_client.rs` — Tonic client for communicating with qarax-node instances
- `qarax/src/vm_monitor.rs` — Background task that periodically reconciles VM status with nodes
- `qarax/src/resource_monitor.rs` — Background task polling host resource metrics
- `qarax/src/transfer_executor.rs` — Async file transfer handling
- `qarax/src/host_deployer.rs` — Host deployment via SSH + bootc
- `qarax/src/configuration.rs` — YAML config with env var overrides

### Data Plane (qarax-node)

- `qarax-node/src/services/vm/` — VM lifecycle gRPC service (create, start, stop, pause, resume, delete)
- `qarax-node/src/services/file_transfer/` — File download/copy gRPC service
- `qarax-node/src/cloud_hypervisor/manager.rs` — Cloud Hypervisor process management
- `qarax-node/src/image_store/manager.rs` — OCI image handling via virtiofsd
- `qarax-node/src/overlaybd/manager.rs` — OverlayBD TCMU block device management

### OpenAPI

Auto-generated via utoipa derive macros → `openapi.yaml`. Swagger UI at `/swagger-ui`. Python SDK in `python-sdk/` regenerated from spec via `make sdk`.

## Key Conventions

### Control Plane Handler Pattern

Handlers in `qarax/src/handlers/` follow a consistent pattern:
- Extract `Extension(env: App)` for DB pool access, request bodies via `Json(model)`
- Return `ApiResponse<T>` wrapping data with a `StatusCode`
- Annotate with `#[utoipa::path(...)]` for OpenAPI generation — all new handlers must include this
- Routes defined declaratively in `handlers/mod.rs` using `Router::new()` with merged sub-routers

### Model Pattern

Models in `qarax/src/model/` use three struct tiers:
- **Domain model** (e.g., `Vm`): `Serialize`/`Deserialize` for API responses
- **Row type** (e.g., `VmRow`): `#[sqlx::FromRow]` for DB results, with `From<Row> for Model` conversions
- **Request type** (e.g., `NewVm`): for POST/PATCH bodies

Queries use `sqlx::query_as!()` with compile-time checking. Column casts are explicit: `status as "status: _"`, `host_id as "host_id?"`.

Database enums use `#[sqlx(type_name = "vm_status")]` with `#[serde(rename_all = "snake_case")]`.

### Error Handling

Custom `Error` enum in `qarax/src/errors.rs` using `thiserror`:
- `Sqlx(sqlx::Error)` → 500, `InvalidEntity(ValidationErrors)` → 422, `Conflict(String)` → 409, `NotFound` → 404
- Smart extraction of unique constraint violations into user-friendly messages
- `Result<T, E = Error>` type alias used throughout the control plane

### Data Plane Service Pattern

gRPC services in `qarax-node/src/services/` implement generated tonic traits:
- Each service struct wraps `Arc<Manager>` for shared state
- Domain errors are mapped to `tonic::Status` via `map_manager_error()`
- Use `#[instrument]` and structured tracing throughout

### App Startup

`qarax/src/startup.rs` wires the app: load config → run migrations → create pool → build `App` (wraps `Arc<PgPool>` + VM defaults) → spawn background tasks (`vm_monitor`, `resource_monitor`) → start Axum with middleware (request ID, tracing).

## Configuration

YAML files in `configuration/` (base.yaml, local.yaml, production.yaml), selected by `APP_ENVIRONMENT` env var (default: local). Key env var overrides: `DATABASE_HOST`, `DATABASE_PORT`, `DATABASE_USERNAME`, `DATABASE_PASSWORD`, `DATABASE_NAME`.

## CI

GitHub Actions (`rust-ci.yml`): fmt check (nightly) → clippy → build (musl) → unit tests → E2E tests. E2E tests run on push to master or PRs with `run-e2e` label. E2E uses pytest against a Docker Compose stack with KVM passthrough (`e2e/`).

## Rules

- **Never modify generated files directly.** Generated files: `python-sdk/` (from `openapi.yaml` via `make sdk`) and `openapi.yaml` (from utoipa annotations via `make openapi`). Fix the source, then regenerate.
- **Never modify production config files for test purposes.** Fix the Dockerfile or compose file instead.
- **Always run `make lint` after changes.** Zero clippy warnings are required. Check all workspace crates, not just the one you edited.
- **Verify changes work before reporting done.** After any Rust code change: `make lint` to confirm zero warnings, then `cargo nextest run -p <crate>` (or `make test`) to confirm tests pass. Do not present a change as complete without verifying it compiles and tests pass.
- **After any SQL query change:** run `cargo sqlx prepare --workspace` to update the offline query cache.
- **When renaming anything:** search `hack/`, `e2e/`, `.github/`, and all workspace crates for stale references before finishing.
- **Plan before implementing on non-trivial changes.** List files to modify and describe the approach. Do not create or edit files until the plan is clear.
- **When making user facing changes** Make sure you have implemented a CLI and added working e2e tests.
- **Never embed credentials, secrets, or personal data in source code.** Use environment variables or config files excluded from version control.
- **When the user redirects or interrupts, stop immediately.** Do not continue the previous approach. Ask a clarifying question if the new direction is unclear.

## Skills

- Use `.github/skills/cloud-hypervisor/SKILL.md` when you need Cloud Hypervisor capabilities or implementation details.
