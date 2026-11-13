# Lifecycle Hooks Demo

Watch webhook notifications arrive as a VM moves through its lifecycle:
created → pending → running → paused → running → shutdown → deleted.

The script starts a small HTTP receiver locally (`receiver.py`), registers a
hook scoped to VMs tagged `demo-hooks` with an HMAC secret, then drives a
tagged VM through start, pause, resume, stop and delete. The receiver prints
each delivery as it arrives and verifies its `X-Qarax-Signature` header
(`sha256=` + HMAC-SHA256 of the raw body, keyed with the hook's secret). The
demo fails if a transition isn't delivered within 15 seconds or a signature
doesn't verify.

Creating a VM is not a status change, so it fires no hook. Starting one fires
two: `created → pending` when the start job is accepted, then
`pending → running` once the VM runs.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `jq` and `python3`
- On Linux, `docker`, to find the address the qarax container reaches this
  machine on (or set `WEBHOOK_HOST`)
- `qarax` CLI: built from this checkout when cargo is available, else taken from `PATH`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/hooks/run.sh

# Custom API endpoint
./demos/hooks/run.sh --server http://localhost:8000

# If the qarax container can't reach the auto-detected address
WEBHOOK_HOST=192.168.1.10 ./demos/hooks/run.sh

# Keep the hook afterwards to inspect `qarax hook executions demo-hooks-webhook`
KEEP=1 ./demos/hooks/run.sh

# Remove what an earlier run left behind
./demos/hooks/run.sh --cleanup
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--webhook-host HOST` | `$WEBHOOK_HOST`, else the `e2e_default` Docker network gateway on Linux, `host.docker.internal` elsewhere | How qarax reaches this machine |
| `--webhook-port PORT` | `$WEBHOOK_PORT` or `9999` | Local port for the webhook receiver |
| `--cleanup` | | Delete the demo's hook and VM, then exit |

## What you'll see

Each delivery carries the VM name, previous and new status, and tags:

```
⚡ WEBHOOK [14:32:01.123] demo-hooks-vm: created→pending  tags=demo-hooks  signature verified
⚡ WEBHOOK [14:32:01.124] demo-hooks-vm: pending→running  tags=demo-hooks  signature verified
⚡ WEBHOOK [14:32:03.130] demo-hooks-vm: running→paused  tags=demo-hooks  signature verified
```

The hook's execution history is shown at the end via `qarax hook executions`.

## Cleanup

Resources are named with the `demo-hooks-` prefix. Each run first deletes
what an earlier run left behind, and on exit stops the receiver and deletes
the hook and VM (unless `KEEP=1`).
