# Lifecycle Hooks Demo

Watch webhook notifications fire in real-time as a VM moves through its lifecycle (created → running → paused → resumed → shutdown → deleted).

Starts a tiny HTTP server locally, registers a global webhook hook, then drives a VM through its full lifecycle while displaying each incoming webhook payload.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `jq` and `python3` installed
- `qarax` CLI on PATH (or Rust toolchain to auto-build it)

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/hooks/run.sh

# Custom API endpoint
./demos/hooks/run.sh --server http://localhost:8000

# If the qarax container can't reach the auto-detected address
WEBHOOK_HOST=192.168.1.10 ./demos/hooks/run.sh
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--webhook-host HOST` | `$WEBHOOK_HOST`, else the `e2e_default` Docker network gateway on Linux, `host.docker.internal` elsewhere | How qarax reaches this machine |
| `--webhook-port PORT` | `$WEBHOOK_PORT` or `9999` | Local port for the webhook receiver |

## What you'll see

Each VM state transition fires a webhook with the VM name, previous status, new status, and tags. The demo script prints them inline as they arrive:

```
⚡ WEBHOOK [14:32:01.123] hooks-demo-12345: created → running
⚡ WEBHOOK [14:32:04.456] hooks-demo-12345: running → paused
```

The hook execution history is shown at the end via `qarax hook executions`.

## Cleanup

The script cleans up automatically on exit (VM deleted, hook removed, webhook receiver stopped).
