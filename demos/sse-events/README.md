# SSE Event Stream Demo

Demonstrates the `GET /events` endpoint which streams VM status-change events
via Server-Sent Events (SSE).

## What it shows

1. Opens two background SSE subscriptions before any VM activity:
   - An unfiltered stream that captures every `vm.status_changed` event
   - A filtered stream (`?status=running`) that captures only transitions to `running`
2. Runs a VM through a full lifecycle: create → start → stop → delete
3. Displays the captured events, showing status transitions with timestamps
4. Prints the raw SSE wire format so you can see the exact protocol framing

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `curl` and `jq` installed
- `qarax` CLI on PATH (or it will be built automatically)

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/sse-events/run.sh
./demos/sse-events/run.sh --server http://localhost:8000
```

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

# Only start events
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
data: {"event":"vm.status_changed","timestamp":"2026-03-26T12:00:00Z","vm_id":"...","vm_name":"...","previous_status":"created","new_status":"running","host_id":"...","tags":[]}
```

The server sends a keep-alive comment (`: keep-alive`) every 15 seconds to
prevent proxy timeouts on idle streams.
