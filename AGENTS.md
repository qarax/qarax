# AGENTS.md

This file provides guidance to AI coding agents working with this repository.

## Project Overview

Qarax is a two-tier VM management platform for Cloud Hypervisor and Firecracker:
- **qarax** (control plane): Axum REST API managing VM/host lifecycle, backed by PostgreSQL
- **qarax-node** (data plane): gRPC service (tonic) running on hypervisor hosts, managing VMs through one backend per hypervisor (Cloud Hypervisor or Firecracker SDK)
- **common**: Shared logging/telemetry helpers
- **cli**: Command-line client (clap)
- **qarax-init**: PID 1 init process for OCI-booted VMs

Communication flow: Control plane → gRPC (`proto/node.proto`) → Data plane → hypervisor API (Cloud Hypervisor or Firecracker, chosen per VM). VMs are scheduled onto hosts in "up" state; subsequent VM operations route to the scheduled host.

## Build & Development Commands

```bash
make build          # Build all + regenerate OpenAPI spec
make test           # Run tests (auto-starts Postgres via Docker)
make test-deps      # Start Postgres in Docker for tests
make lint           # cargo clippy --workspace -- -D warnings
make fmt            # cargo fmt + shfmt (all tracked *.sh)
make openapi        # Regenerate openapi.yaml
make sdk            # Regenerate Python SDK from OpenAPI
make run-local      # Start the Docker stack (qarax + qarax-node + Postgres + registry), register local-node, create overlaybd-pool
make run-local VM=1 # Same, but the node is the appliance image as a bootc microVM (compose service bootc-vm)
make stop-local      # Stop and remove the local Docker stack
make appliance-build # Build bootc container image for qarax-node (hypervisor host)
make appliance-push  # Push appliance image to registry
make ruff-check      # Lint the Python SDK (run after make sdk)
make ruff-fmt        # Auto-format Python SDK with ruff
SKIP_DOCKER=1 make test  # Run tests skipping Postgres startup (if already running)
```

To verify changes, run: `make fmt lint sdk build openapi test`.

Build/test a single crate: `cargo build -p qarax-node` or `cargo nextest run -p qarax -E 'test(test_name)'`

**macOS note:** The default build target is `x86_64-unknown-linux-musl` (in `.cargo/config.toml`), but the Makefile auto-detects macOS and overrides to the host target. To build manually on macOS, pass `--target` explicitly or use `make build`.

**CI uses nightly for fmt:** `cargo +nightly fmt --all -- --check`

**Build dependencies:** `protobuf-compiler` (protoc) is required for gRPC code generation. `shfmt` is required by `make fmt` for shell script formatting; `make shellcheck` needs `shellcheck` (CI runs both through the `ShellLint` Dagger check).

## Database

PostgreSQL 16+ with SQLx (compile-time verified queries).

- Migrations in `migrations/`, run automatically on startup
- Start DB: `make test-deps` or `./scripts/start_db.sh`
- Default connection (local): `qarax:qarax@127.0.0.1:5432/qarax`; CI uses `postgres:password@localhost:5432/qarax`
- After modifying any SQL query: `cargo sqlx prepare --workspace`
- Offline/CI builds: `SQLX_OFFLINE=true cargo build`

## Deployment Topology

Qarax supports two deployment modes — understand which one is in use before debugging:

- **Docker Compose (e2e / local dev):** `qarax` (control plane), `qarax-node`, and `postgres` run as separate containers. `qarax-node` runs privileged with `/dev/kvm` passthrough for real VMs.
- **bootc node microVM (`make run-local VM=1`, e2e upgrade tests):** the compose stack, with the node being the appliance image installed with bootc and booted by Cloud Hypervisor in the `bootc-vm` service (guest `10.0.2.15`; `docker compose exec bootc-vm ssh root@10.0.2.15`). Built by `e2e/build-bootc-node.sh`; see `docs/DEVELOPMENT.md`.
- **Hyperconverged (`demos/hyperconverged`):** a single Cloud Hypervisor VM hosts both the control plane and the node, on the developer's host.

Always confirm the deployment mode before debugging. Many issues (networking, storage mount paths, device availability) differ between the two modes.

## Debugging Protocol

Follow this order before touching code:

1. **Confirm the deployment mode** (container vs hyperconverged VM — see above).
2. **Check that required services are running** before looking at code:
   - Docker Compose mode: `docker compose ps` and `docker compose logs qarax-node`
   - Hyperconverged mode: `systemctl status qarax-node`, `ps aux | grep cloud-hypervisor`
   - Verify Postgres: `docker compose ps postgres` or `docker compose exec postgres pg_isready -U qarax`
   - Verify API auth: `curl -H "Authorization: Bearer $QARAX_TOKEN" localhost:8000/hosts`. A 401 means a wrong/missing token, not a down stack (`/` is public, so it can't tell you).
   - Verify host kernel modules: qarax-node (even in a container) loads `target_core_user` and `tcm_loop` (and nftables modules for its iptables-nft rules) from the host kernel on demand. If `uname -r` has no matching `/lib/modules/` dir (kernel upgraded without reboot), anything not already loaded fails, e.g. TCMU configfs `ENOENT`. Reboot; it is not a code bug.
3. **Read service logs** before grep-ing source files. Most runtime failures are infrastructure, not code.
4. **Only after confirming infrastructure is healthy**, investigate code-level issues.

Do not jump to code investigation when services may not be running or the wrong environment is assumed.

## Architecture

### Control Plane (qarax)

- `qarax/src/handlers/` — Axum HTTP handlers by resource (hosts, vms, instance_types, vm_templates, storage_pools, storage_objects, boot_sources, networks, security_groups, transfers, lifecycle_hooks, jobs, sandboxes, backups, events (SSE), audit_log)
- `qarax/src/auth.rs` — Bearer-token middleware (see API Authentication)
- `qarax/src/secret_provider.rs` — Resolves host `credential_ref`s (`env://NAME`, `file:///path`); passwords are never stored in the DB
- `qarax/src/model/` — Database models with inline SQLx queries
- `qarax/src/grpc_client.rs` — Tonic client for communicating with qarax-node instances
- `qarax/src/vm_monitor.rs` — Background task that periodically reconciles VM status with nodes
- `qarax/src/resource_monitor.rs` — Background task polling host resource metrics
- `qarax/src/ha_monitor.rs` — Background task failing over HA-enabled VMs from dead hosts
- `qarax/src/hook_executor.rs` — Background task managing lifecycle hook executions
- `qarax/src/sandbox_reaper.rs` / `sandbox_pool_manager.rs` — Background tasks reaping idle sandboxes and keeping prewarmed pools filled
- `qarax/src/transfer_executor.rs` — Async file transfer handling
- `qarax/src/host_deployer.rs` — Host deployment via SSH + bootc
- `qarax/src/configuration.rs` — YAML config with env var overrides

### Data Plane (qarax-node)

- `qarax-node/src/services/vm/` — VM lifecycle gRPC service (create, start, stop, pause, resume, delete)
- `qarax-node/src/services/file_transfer/` — File download/copy gRPC service
- `qarax-node/src/vmm/` — `VmmManager` trait every hypervisor backend implements; the VM service dispatches on the VM's hypervisor type
- `qarax-node/src/cloud_hypervisor/manager.rs` — Cloud Hypervisor process management
- `qarax-node/src/firecracker/` — Firecracker backend
- `qarax-node/src/overlaybd/manager.rs` — OverlayBD TCMU block device management
- `qarax-node/src/networking/` — Network management (bridge creation, DHCP allocation, nftables rules)

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

Every query with fixed SQL uses the checked macros (`query!`, `query_as!`, `query_scalar!`). Runtime `sqlx::query*` and `#[derive(sqlx::FromRow)]` are only for SQL assembled with `QueryBuilder` (scheduler `pick_host_tx`, audit log filters). Overrides the macros need:
- Enum and JSON columns: `status as "status: _"`, `config as "config: _"` (`query!` names the type: `"status: VmStatus"`).
- Nullable columns, expressions and casts read into a non-`Option` field: `count as "count!"`, `subnet::text AS "subnet!"`.
- Enum params: an identity cast of the argument's type, e.g. `status as VmStatus`, `&req.scope as &HookScope`, `filter as Option<BackupType>`.
- `Option<String>` params: pass `x.as_deref()`, not `&x`. CIDR/INET/MACADDR params arrive as text: `$1::text::inet`.

Database enums use `#[sqlx(type_name = "vm_status")]` with `#[serde(rename_all = "snake_case")]`.

### Error Handling

Custom `Error` enum in `qarax/src/errors.rs` using `thiserror`:
- `Sqlx(sqlx::Error)` → 500, `InvalidEntity(ValidationErrors)` and `UnprocessableEntity(String)` → 422, `Conflict(String)` → 409, `NotFound` → 404
- Smart extraction of unique constraint violations into user-friendly messages
- `Result<T, E = Error>` type alias used throughout the control plane

### Data Plane Service Pattern

gRPC services in `qarax-node/src/services/` implement generated tonic traits:
- Each service struct wraps `Arc<Manager>` for shared state
- Domain errors are mapped to `tonic::Status` at the service boundary (e.g. `map_vmm_error()` in `services/vm/mod.rs`)
- Use `#[instrument]` and structured tracing throughout

### App Startup

`qarax/src/startup.rs` wires the app: load config → run migrations → create pool → build `App` (wraps `Arc<PgPool>` + VM defaults) → spawn background tasks (`vm_monitor`, `resource_monitor`, `ha_monitor`, `hook_executor`, `sandbox_reaper`, `sandbox_pool_manager`) → start Axum with middleware (request ID, tracing).

## Configuration

YAML files in `configuration/` (base.yaml, local.yaml, production.yaml), selected by `APP_ENVIRONMENT` env var (default: local). Key env var overrides: `DATABASE_HOST`, `DATABASE_PORT`, `DATABASE_USERNAME`, `DATABASE_PASSWORD`, `DATABASE_NAME`.

### API Authentication

Static bearer tokens, configured by `AUTH_ENABLED` / `AUTH_TOKENS` (comma-separated):

- **Off** by default (`base.yaml`; `local.yaml` doesn't override it), e.g. `cargo run` against `make test-deps`.
- **On** in `production.yaml` (no default tokens: set `AUTH_TOKENS`) and in `e2e/docker-compose.yml`, which is also what `make run-local` uses. There the token is `${QARAX_TEST_TOKEN:-e2e-test-token}`.
- Public routes: `/`, `/swagger-ui*`, `/api-docs/openapi.json`. Everything else needs `Authorization: Bearer <token>`.
- The CLI reads `--token` / `QARAX_TOKEN`. `demos/lib.sh` (sourced by `hack/run-local.sh` and `e2e/setup_host.sh`) and `e2e/run_e2e_tests.sh` default it to `e2e-test-token`.

## CI

CI is driven by Dagger (`ci/main.go`) invoked from GitHub Actions (`.github/workflows/rust-ci.yml`).

### Pipeline structure

Three jobs run on every PR and push to master:

- **check** (`dagger call --mod ./ci all --src=.`): runs fmt, lint, test, sqlx-check, audit, openapi-check, python-sdk-lint and shell-lint in parallel. Lint, test, openapi-check and python-sdk-lint (through the generated spec) chain from a shared `prebuilt` container (debug build with the otel features), so the workspace compiles once per session; fmt, sqlx-check, audit and shell-lint run in their own containers.
- **Caching:** hosted runners start with empty Dagger cache volumes, so Rust compiles start from scratch on every run (per-object caching through the Actions cache, e.g. sccache's GHA backend, gets rate-limited at this repo's ~2,400 compile outputs per run). The e2e images use BuildKit's GitHub Actions cache (`e2e/docker-compose.ci.yml`), whose entries are evicted after 7 days without use.
- **build** (`dagger call --mod ./ci build --src=.`): release build targeting `x86_64-unknown-linux-musl`; uploads the four binaries as a workflow artifact.
- **e2e**: downloads the build artifacts, starts a Docker Compose stack with KVM passthrough, runs pytest. Triggered on push to master inside `rust-ci.yml`, or in a dedicated PR E2E workflow when the `run-e2e` label is first added. After that, labeled PR updates run E2E from `rust-ci.yml` on `synchronize`.

### Dagger checks

| Function | What it does |
|---|---|
| `Fmt` | `cargo +nightly fmt --all -- --check` |
| `Lint` | `cargo clippy --workspace --features qarax/otel,qarax-node/otel -- -D warnings` |
| `Test` | `cargo nextest run --workspace --features qarax/otel,qarax-node/otel` against a live Postgres 16 service |
| `SqlxCheck` | `cargo sqlx prepare --workspace --check` after running migrations |
| `Audit` | `cargo audit` (ignores configured in `.cargo/audit.toml`) |
| `OpenApiCheck` | Regenerates `openapi.yaml` and diffs against the committed copy |
| `PythonSdkLint` | Regenerates the Python SDK and runs `ruff check` |
| `ShellLint` | `shellcheck --severity=warning` and `shfmt -d` over every `*.sh` (nothing else exercises `demos/` and `hack/`) |

### Running CI locally

```bash
# Run all checks (same as CI):
dagger call --mod ./ci all --src=.

# Run a single check:
dagger call --mod ./ci fmt --src=.
dagger call --mod ./ci lint --src=.
dagger call --mod ./ci test --src=.
dagger call --mod ./ci sqlx-check --src=.
dagger call --mod ./ci audit --src=.
dagger call --mod ./ci open-api-check --src=.
dagger call --mod ./ci python-sdk-lint --src=.
dagger call --mod ./ci shell-lint --src=.

# Build release binaries via Dagger:
dagger call --mod ./ci build --src=. export --path=./target/x86_64-unknown-linux-musl/release
```

### Advisory ignores

Known unfixable advisories are listed in `.cargo/audit.toml` with comments explaining why each is ignored and what upstream change would unblock removal.

### Dependency cooldown

`.cargo/config.toml` sets `registry.global-min-publish-age = "7 days"`, so Cargo won't resolve a crates.io version published less than a week ago unless it's already in `Cargo.lock`; Renovate's `minimumReleaseAge` matches it. When an advisory fix is younger than that, bump it explicitly: `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo update -p <crate> --precise <version>`.

### E2E trigger

E2E tests run automatically on push to master. On PRs, apply the `run-e2e` label to trigger a dedicated PR E2E workflow without re-running the other Rust CI jobs; future commits to that labeled PR still run E2E on `synchronize`.

## Versioning

Cloud Hypervisor and Firecracker versions are pinned in `versions/` (the `Makefile` reads them and passes them to `deployments/Containerfile.qarax-vmm` as build args); OverlayBD, the ACI convertor, CLOUDHV.fd and the guest kernel are pinned in `versions/node-artifacts.env`, which the node Containerfiles read through a build-time bind mount. The Cloud Hypervisor and Firecracker SDKs are pinned via git tags in `qarax-node/Cargo.toml`; their repos (`qarax/cloud-hypervisor-sdk`, `qarax/firecracker-rust-sdk`) regenerate and tag a release automatically when upstream ships one (CH `vX.Y` becomes SDK `vX.Y.0`, Firecracker tags are used as is). Bump the matching `versions/` file together with each SDK tag (`cloud-hypervisor-version` with the CH SDK, `firecracker-version` with the Firecracker SDK), since each SDK serializes that release's API. After bumping an SDK tag, always run `cargo build -p qarax-node`: releases can rename/move types (e.g., v54 moved `disk_config::ImageType` to `models::ImageType` and turned the device `iommu` flags into `IommuType`).

## E2E Test Patterns

- **Async fixture clients**: Pytest async fixtures that use `async with Client(...) as c:` must create their own `Client(base_url=QARAX_URL)` — never share the test's `client` fixture. httpx clients cannot be opened as a context manager more than once per instance.
- **`docker compose` commands**: Run from `e2e/` (or pass `-f e2e/docker-compose.yml`). `make run-local` and the e2e runner share the compose project `e2e`.
- **Compose profiles**: the default stack (`make run-local`) is registry, postgres, qarax-node and qarax. `nfs-server` (profile `e2e`), `qarax-node-2` (`multi-node`) and `bootc-vm` (`bootc`) start only when named, as `run_e2e_tests.sh` does, or on demand (`ensure_up_hosts 2` in `demos/lib.sh`). Tear down with `docker compose --profile '*' down -v`.
- **Compose Postgres is internal-only**: the E2E stack doesn't bind Postgres to a host port, so it can run alongside the standalone test database container.
- **Pinned node artifacts**: OverlayBD, ACI, CLOUDHV.fd and guest kernel versions live in `versions/node-artifacts.env`; the node Containerfiles read it through a `RUN --mount=type=bind`, so bump them there.
- **`call_api` raises on any status >= 400**; use `call_api_detailed` when a test expects an error status.
- **Never `pytest.skip` on a product failure** (a VM that didn't reach RUNNING, a missing route): that turns a regression into a green run. Skip only on environment gates such as "needs two UP hosts".

## Test Ownership

E2E runs only on master and labelled PRs, so a contract that needs no real VM or node belongs in a Rust test that runs on every PR.

- **REST contracts** (status codes, error messages, validation guards, persistence through the API): `qarax/tests/*.rs`, using `qarax/tests/support` (`spawn_app`, `add_up_host`, `create_vm`, `set_vm_status` for states only a node could reach).
- **SQL and scheduling logic**: `#[cfg(test)]` modules using `crate::test_support::TestDatabase`.
- **E2E**: behaviour that needs KVM, a hypervisor, guest networking or control plane to node gRPC.
- Test a shared helper once, in the crate that owns it (e.g. `common::cpu_list`). See `.claude/skills/test-audit/SKILL.md` before adding or changing tests.

## Additional Directories

- `demos/` — Runnable demos, one per directory (`run.sh` + `README.md`); see `demos/README.md` for the index, stack requirements and the conventions every demo follows (fixed name prefix swept at start, `KEEP=1`, `--cleanup`). Shared helpers (output, `ensure_stack`, `require_cli` + `qarax`, `ensure_up_hosts`, `wait_until`, `start_vms`, `sweep`, ...) live in `demos/lib.sh`; don't redefine them in a demo.
- `docs/` — Additional development documentation (`DEVELOPMENT.md`)

## Rules

- **Never modify generated files directly.** Generated files: `python-sdk/` (from `openapi.yaml` via `make sdk`) and `openapi.yaml` (from utoipa annotations via `make openapi`). Fix the source, then regenerate.
- **Never modify production config files for test purposes.** Fix the Dockerfile or compose file instead.
- **Always run `make lint` after changes.** Zero clippy warnings are required. Check all workspace crates, not just the one you edited.
- **Verify changes work before reporting done.** After any Rust code change: `make lint` to confirm zero warnings, then `cargo nextest run -p <crate>` (or `make test`) to confirm tests pass. Do not present a change as complete without verifying it compiles and tests pass.
- **After any SQL query change:** run `cargo sqlx prepare --workspace` to update the offline query cache.
- **When renaming anything:** search `hack/`, `e2e/`, `demos/`, `.github/`, and all workspace crates for stale references before finishing.
- **When changing an API request shape, CLI flag, or auth behavior:** grep `demos/` and `hack/` for raw callers (`curl`, python `requests`, CLI invocations). Nothing in CI exercises them, so they break silently. Some request types (e.g. `NewHost`) use `#[serde(deny_unknown_fields)]`, so removing a field from them turns old callers into 422s.
- **When making user facing changes** Make sure you have implemented a CLI and added working e2e tests.
- **Never embed credentials, secrets, or personal data in source code.** Use environment variables or config files excluded from version control.
- **When the user redirects or interrupts, stop immediately.** Do not continue the previous approach. Ask a clarifying question if the new direction is unclear.

## General Guidelines

- Always push to 'upstream' remote, not 'origin', unless explicitly told otherwise.
- When fixing CI/test failures, check every required field and parameter in one pass rather than fixing them one at a time. Review the full struct/function signature before submitting.
- Read existing codebase conventions (demo structure, file locations, patterns) before creating new files. Do not assume conventions - use Grep/Read to discover them first.


## Skills

- Use `.claude/skills/cloud-hypervisor/SKILL.md` when you need Cloud Hypervisor capabilities or implementation details.
- Use `.claude/skills/demo-stabilization/SKILL.md` when a demo or bring-up workflow is failing and you need a structured debug path before editing code.
- Use `.claude/skills/ci-debug/SKILL.md` when a CI job or PR check fails, including when asked to watch a replacement run.
- Use `.claude/skills/test-audit/SKILL.md` when writing, changing, reviewing or auditing tests.
