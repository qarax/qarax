# Sandbox Demo

Ephemeral VMs on demand: create a sandbox from a template, use it, and let it expire.

A **sandbox** is a single-use VM with an idle timeout. When it has seen no
activity (`exec` or `keepalive`) for longer than the timeout, the reaper
deletes it and its VM.

## What this demo shows

1. Get or create two templates for the same guest: `sandbox-demo-fc` (Firecracker) and `sandbox-demo-ch` (Cloud Hypervisor)
2. Create a sandbox from each and time how long each takes from `sandbox create` to `ready`
3. Configure a prewarmed sandbox pool for the Firecracker template
4. Inspect the Firecracker sandbox, reset its idle clock with `keepalive` and run `uname -a` inside it with `sandbox exec`
5. Claim a second sandbox from the pool and time it (the demo checks that it was claimed, not booted)
6. Delete the first sandbox by hand
7. Wait for the reaper to delete the second sandbox after its idle timeout (30 s by default)

The summary lists the three times. They are end-to-end numbers from one run
(the demo polls the sandbox every 0.1 s), useful for comparing backends in this
environment, not a rigorous benchmark.

## Prerequisites

- The qarax stack: started with `hack/run-local.sh` (same as `make run-local`) if it isn't running. `--cleanup` only needs it to be reachable.
- `cargo` (the script builds the CLI) or a `qarax` CLI on `PATH`
- `jq`, `curl`, and bash 5 or newer
- `QARAX_TOKEN` matching the server's API token; it defaults to the local stack's `e2e-test-token`

The demo templates have no boot source: their VMs boot the control plane's
default kernel and initramfs (`VM_KERNEL` and `VM_INITRAMFS`, which on the local
stack is `test-initramfs.gz`). That initramfs runs `qarax-init`, the guest agent
`sandbox exec` talks to. A template passed with `--template` needs the same.

## Usage

```bash
# Firecracker vs Cloud Hypervisor cold start, then a prewarmed pool
./demos/sandbox/run.sh

# Use an existing template (skips the Cloud Hypervisor comparison)
./demos/sandbox/run.sh --template my-template

# Give the pooled sandbox a longer idle timeout before it is reaped
./demos/sandbox/run.sh --idle-timeout 60

# Leave the sandboxes and the pool in place at exit
KEEP=1 ./demos/sandbox/run.sh

# Remove the demo's sandboxes, pools and templates
./demos/sandbox/run.sh --cleanup
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--template NAME` | `sandbox-demo-fc` | Use an existing VM template. The demo doesn't delete it, and restores the `min_ready` of a sandbox pool it already had. If that pool has a standby ready, the first sandbox is claimed from it rather than booted. |
| `--idle-timeout SECS` | `$SANDBOX_IDLE_TIMEOUT` or `30` | Idle timeout of the sandbox claimed from the pool, the one the demo waits to see reaped. The other sandboxes use the server's default (300 s). |
| `--cleanup` | | Remove the demo's sandboxes, sandbox pools and templates, then exit |

Environment variables:

- `KEEP=1`: leave the `sandbox-demo-*` sandboxes and the sandbox pool in place at exit
- `SANDBOX_POOL_MIN_READY` (default `1`): standby sandboxes the pool keeps ready
- `SANDBOX_READY_TIMEOUT` (default `300`): seconds to wait for a sandbox to be ready, and for the pool to fill
- `QARAX_SERVER`, `SANDBOX_IDLE_TIMEOUT`: defaults for the matching flags

## How auto-reap works

The `sandbox_reaper` background task runs every 10 seconds
(`sandbox.reaper_interval_secs`) and finds ready sandboxes where
`last_activity_at + idle_timeout_secs < NOW()`. Each one moves to `destroying`,
and its VM is stopped and deleted along with the sandbox.

`qarax sandbox exec` and `qarax sandbox keepalive` (POST
`/sandboxes/{id}/keepalive`) reset the idle clock. `qarax sandbox get` doesn't,
so monitoring a sandbox doesn't keep it alive. A sandbox claimed from a pool
starts its idle clock at the claim.

## Cleanup

On exit, including when interrupted, the demo deletes its sandboxes and the
sandbox pool it configured (unless `KEEP=1`). The two templates are kept, so the
next run reuses them. A run also clears sandboxes and pools that an interrupted
run left behind. To remove everything, templates included:

```bash
./demos/sandbox/run.sh --cleanup
```
