# SSE Event Stream Demo

Demonstrates the `GET /events` endpoint, which streams VM status changes as
Server-Sent Events (SSE).

## What it shows

1. Opens two SSE subscriptions before any VM activity:
   - the full stream, which receives every `vm.status_changed` event
   - a filtered stream (`?status=running`), which receives only transitions to `running`
2. Creates a VM, then starts, pauses, resumes and stops it, printing each event
   as it arrives on each stream:

   | Step | Full stream | `?status=running` |
   |------|-------------|-------------------|
   | create | (no event: creation is not a status change) | |
   | start | `created→pending`, `pending→running` | `pending→running` |
   | pause | `running→paused` | |
   | resume | `paused→running` | `paused→running` |
   | stop | `running→shutdown` | |

3. Checks that each stream delivered exactly those transitions for the demo VM
   (the demo fails otherwise), then prints one event in its raw wire format.

Deleting a VM emits no SSE event (lifecycle hooks do receive a `deleted`
delivery; see `demos/hooks`), so the VM is deleted on exit rather than as a
lifecycle step.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `curl` and `jq`
- `qarax` CLI: built from this checkout when cargo is available, else taken from `PATH`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/sse-events/run.sh
./demos/sse-events/run.sh --server http://localhost:8000

# Leave the VM (demo-sse-vm) in place
KEEP=1 ./demos/sse-events/run.sh

# Remove what an earlier run left behind
./demos/sse-events/run.sh --cleanup
```

Each run first deletes VMs left by an earlier run (names start with `demo-sse-`).

## Query parameter filters

`/events` is authenticated like the rest of the API, so pass the bearer token
(the local stack's default is `e2e-test-token`). The endpoint accepts optional
query parameters to narrow the event stream before it reaches your client:

| Parameter | Description |
|-----------|-------------|
| `vm_id`   | Only events for the specified VM UUID |
| `status`  | Only events transitioning **to** this status (e.g. `running`, `shutdown`) |
| `tag`     | Only VMs carrying this tag |

Examples:

```bash
# All events
curl -N -H "Authorization: Bearer $QARAX_TOKEN" http://localhost:8000/events

# Only transitions to running
curl -N -H "Authorization: Bearer $QARAX_TOKEN" 'http://localhost:8000/events?status=running'

# Events for a single VM
curl -N -H "Authorization: Bearer $QARAX_TOKEN" 'http://localhost:8000/events?vm_id=<uuid>'

# Combined filter: only a specific VM transitioning to shutdown
curl -N -H "Authorization: Bearer $QARAX_TOKEN" 'http://localhost:8000/events?vm_id=<uuid>&status=shutdown'
```

## Wire format

```
event: vm.status_changed
id: <vm-uuid>
data: {"event":"vm.status_changed","timestamp":"2026-03-26T12:00:00.123456789+00:00","vm_id":"...","vm_name":"...","previous_status":"pending","new_status":"running","host_id":"...","tags":[]}
```

The server sends a keep-alive comment every 15 seconds to prevent proxy
timeouts on idle streams.
