# Qarax E2E Tests

End-to-end tests for the Qarax VM management platform, run with pytest and
the generated Python SDK (`qarax-api-client`) against a Docker Compose stack.

They own what the Rust suites cannot prove: real Cloud Hypervisor and
Firecracker VMs under KVM, guest networking, storage on real nodes, and the
control plane to node gRPC path. REST status codes, validation errors and
persistence round trips belong in `qarax/tests/*.rs`, which run on every PR.

## Stack

`run_e2e_tests.sh` starts these compose services and registers both nodes as
UP hosts (`setup_host.sh`) before pytest runs. `nfs-server`, `qarax-node-2` and
`bootc-vm` sit behind compose profiles, so `make run-local` (same compose
project) leaves them out; the runner names them, which enables them:

- **qarax**: control plane on `:8000`, with API token auth enabled
- **qarax-node** and **qarax-node-2**: data plane nodes with `/dev/kvm`,
  Cloud Hypervisor and Firecracker (versions from `../versions/`)
- **postgres**: database (internal to the compose network)
- **nfs-server**: shared NFS export for shared-storage tests
- **registry**: local OCI registry for OverlayBD image tests
- **bootc-vm** (with `ENABLE_BOOTC_UPGRADE_E2E=1`): the appliance image as a
  Cloud Hypervisor microVM node for `test_node_upgrade.py`, built by
  `build-bootc-node.sh` (see `docs/DEVELOPMENT.md`)

VMs boot the Cloud Hypervisor test kernel with a BusyBox initramfs that runs
`qarax-init` and dropbear, built in `Dockerfile.qarax-node`.

## Prerequisites

- Docker with Compose v2
- `/dev/kvm` readable and writable by your user
- [uv](https://docs.astral.sh/uv/)
- Rust toolchain (the runner builds the release musl binaries on every run;
  cargo is a no-op when they're current)

## Running

```bash
cd e2e
./run_e2e_tests.sh                       # whole suite
./run_e2e_tests.sh test_vm_lifecycle.py  # one file
./run_e2e_tests.sh -k live_migration     # by expression
```

If a stack is already running on `:8000` (e.g. from `make run-local`), the
runner reuses the compose project and leaves the stack up afterwards, as with
`KEEP=1`.

| Variable | Description |
|----------|-------------|
| `REBUILD` | Rebuild the Docker images without cache |
| `KEEP` | Keep services running after the tests |
| `SKIP_BUILD` | Use the existing release binaries (CI downloads them) |
| `ENABLE_OTEL` | Build with `otel` and run the telemetry test against a local OTLP receiver |
| `ENABLE_BOOTC_UPGRADE_E2E` | Build a bootc VM and run `test_node_upgrade.py` |

Every test has a 600 s pytest-timeout (`pyproject.toml`), and skip reasons are
printed in the summary (`-rs`).

## Test Overview

| File | Covers |
|------|--------|
| `test_auth.py` | The deployed API rejects requests without a token |
| `test_backups.py` | VM and database backups: create, restore, delete removes stored data |
| `test_cloud_init.py` | Cloud-init seed image written on start and removed on delete (Cloud Hypervisor) |
| `test_disk_resize.py` | Disk resize on a Shutdown VM grows the backing file on the node |
| `test_firecracker.py` | Firecracker lifecycle, force stop, cloud-init, snapshot/restore, unsupported disk hotplug, host versions |
| `test_gpu_scheduling.py` | A VM requesting a GPU is rejected when no host has one |
| `test_ha_failover.py` | HA VMs restart on the surviving host when a node dies; non-HA VMs stay |
| `test_hotplug.py` | Disk and NIC hotplug on running and Shutdown VMs |
| `test_lifecycle_hooks.py` | Webhooks fire for a VM's status transitions |
| `test_live_migration.py` | Live migration with a shared NFS disk, host evacuation, placement policies |
| `test_network.py` | Isolated networks, guest connectivity, cross-host VPC routing and security groups |
| `test_node_upgrade.py` | bootc deploy and upgrade of the appliance as a Cloud Hypervisor microVM node (`ENABLE_BOOTC_UPGRADE_E2E=1`, which CI sets) |
| `test_numa.py` | NUMA topology discovery and NUMA-pinned VM boot |
| `test_resize.py` | vCPU and memory hotplug on a running VM |
| `test_sandboxes.py` | Sandbox create, exec, delete, warm pools and idle reaping |
| `test_snapshots.py` | Snapshot a running VM and restore it |
| `test_storage_safety.py` | Pool capacity accounting and node file cleanup for local and NFS pools |
| `test_storage_types.py` | OCI images via OverlayBD: create, preflight, persistent upper layer, commit |
| `test_telemetry.py` | OTLP trace propagation and metrics (needs `ENABLE_OTEL=1`) |
| `test_transfers.py` | Local copy transfer into a storage pool |
| `test_vm_lifecycle.py` | VM lifecycle, start/stop cycles, force stop, guest exec, monitor tick |

## Writing Tests

- Use `helpers.call_api` for calls that must succeed (it raises on any HTTP
  error) and `helpers.call_api_detailed` when the test checks an error status.
- Wait for states with `helpers.wait_for_status` / `wait_for_host_status`,
  never a fixed sleep. Never `pytest.skip` on a product failure.
- Async fixtures that open a client must create their own `Client`; an httpx
  client can only be opened once.
- Run `docker compose` commands from `e2e/`.
- Lint with `uv run --with ruff ruff check .`.

## Debugging

```bash
KEEP=1 ./run_e2e_tests.sh
docker compose logs qarax-node
docker compose exec qarax-node sh
curl -H "Authorization: Bearer e2e-test-token" http://localhost:8000/vms
docker compose --profile '*' down -v
```
