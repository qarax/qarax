# Sandbox Demo

Ephemeral VMs on demand — spin up a sandbox from a template, use it, and let it auto-expire.

A **sandbox** is a single-use VM with an idle timeout.  When there has been no
activity (`exec` or `keepalive`) within the timeout window, the reaper
automatically stops and deletes the underlying VM.  No manual cleanup required.

## What this demo shows

1. Create two VM templates backed by the same boot source (Firecracker default + Cloud Hypervisor comparison)
2. Provision one sandbox from each template and measure cold time-to-ready
3. Configure a prewarmed Firecracker sandbox pool and wait for a standby sandbox
4. Inspect the Firecracker sandbox that represents the default sandbox path
5. Execute a command inside the Firecracker sandbox over the guest agent
6. Claim a second Firecracker sandbox from the prewarmed pool and measure warm time-to-ready
7. Delete one sandbox manually
8. Watch the remaining sandbox get auto-reaped after its idle timeout expires

## Prerequisites

- qarax stack: started automatically via `hack/run-local.sh` (same as `make run-local`) if it is not already running; `--cleanup` only requires it to be reachable
- `cargo` (the script builds the CLI first; set `SKIP_BUILD=1` to use an existing build) and `python3`
- A host registered and initialized (run-local.sh does this automatically; override the host name with `QARAX_HOST`, default `local-node`)
- `QARAX_TOKEN` must match the server's API token; it defaults to the local stack's `e2e-test-token`
- The demo requires an initramfs that contains `qarax-init`, because it runs `qarax sandbox exec` inside the guest
- Firecracker is the default managed sandbox backend in this demo, and the exec step now runs against the Firecracker sandbox itself
- By default it uses `/var/lib/qarax/images/test-initramfs.gz` from the local e2e/demo environment; override with `--initramfs PATH` or `SANDBOX_INITRAMFS_PATH`
- The default run removes old `sandbox-demo-*` sandboxes, VMs, sandbox pools, and template assets left by interrupted runs, creates fresh per-run template assets, and configures a one-entry prewarmed sandbox pool before the warm-claim step

## Usage

```bash
# Default: creates Firecracker + Cloud Hypervisor demo templates, benchmarks both, then configures a one-entry warm pool
./demos/sandbox/run.sh

# Custom server
./demos/sandbox/run.sh --server http://localhost:8000

# Reuse an existing template that boots with an exec-capable initramfs
./demos/sandbox/run.sh --template my-template

# Shorter idle timeout to see auto-reap faster
./demos/sandbox/run.sh --idle-timeout 30

# Remove demo-managed resources only
./demos/sandbox/run.sh --cleanup
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--template NAME` | demo-managed `sandbox-demo-template-*` | Reuse an existing VM template; it is not deleted by the demo, and an existing sandbox pool on it is restored to its original `min_ready` on exit. When this is used, the built-in Firecracker vs Cloud Hypervisor benchmark is skipped. |
| `--idle-timeout SECS` | `90` | Idle timeout before auto-reap |
| `--initramfs PATH` | `/var/lib/qarax/images/test-initramfs.gz` | Initramfs path on the qarax-node host; must contain `qarax-init` |
| `--cleanup` | n/a | Remove demo-managed sandboxes, VMs, and template assets, then exit |

Environment variables:

- `SANDBOX_POOL_MIN_READY` (default `1`): standby sandboxes kept ready before the warm claim
- `SANDBOX_READY_TIMEOUT` (default `300`): seconds to wait for a sandbox to reach `ready`
- `SANDBOX_IDLE_TIMEOUT`, `SANDBOX_INITRAMFS_PATH`, `QARAX_SERVER`: defaults for the matching flags

## How auto-reap works

The `sandbox_reaper` background task runs every 10 seconds by default
(`sandbox.reaper_interval_secs`) and queries for
sandboxes where `last_activity_at + idle_timeout_secs < NOW()`.  Any matching
sandbox transitions to `destroying` and its VM is stopped and deleted.

To reset the idle clock without fetching the full record, call
`qarax sandbox keepalive <id>` (POST `/sandboxes/{id}/keepalive`). Plain
`qarax sandbox get` no longer extends the lifetime — that side effect was
removed so monitoring tools don't accidentally keep sandboxes alive.
`qarax sandbox exec` continues to count as activity.

## Benchmark notes

The reported cold Firecracker vs Cloud Hypervisor numbers and the prewarmed claim number are simple end-to-end demo measurements from `sandbox create` until the sandbox reaches `ready`. They are useful for comparing this environment, but they are not a rigorous microbenchmark.

## Cleanup

The demo cleans up after itself (sandboxes, sandbox pool config, and template assets) on exit,
including when interrupted. The shared `sandbox-demo-pool` storage pool is kept for reuse.
If a run was killed before its exit trap could run:

```bash
./demos/sandbox/run.sh --cleanup
```
