# Host Maintenance / Evacuation Demo

Demonstrates Qarax's manual host evacuation workflow on a two-host stack:

- make sure two hosts are `up`
- create and start a small live-migration-compatible VM
- evacuate the VM's host with `qarax host evacuate`
- show the host ends in `maintenance` and the VM is running on the other host
- create another VM and show new placement avoids the maintenance host

## Prerequisites

- local stack from `make run-local` (the script starts it via
  `hack/run-local.sh` if the API is not reachable). If only one host is UP,
  the script starts the second node container (`qarax-node-2`) and registers
  it as `local-node-2` (with `e2e/setup_host.sh`, which needs `python3`).
  Against another server, two hosts must already be UP.
- `jq`
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it

If your server uses a token other than the local default, export `QARAX_TOKEN`.

At start the script returns **every** host in `maintenance` to `up`: an
interrupted run leaves its evacuated host in maintenance, and the demo needs
both hosts. Don't point it at a stack where a host is in maintenance on purpose.

Evacuation only moves running or paused VMs; if the source host also runs VMs
in another state (for example `created` VMs left by other demos), the API
refuses the evacuation and names them.

## Usage

```bash
./demos/host-evacuation/run.sh

# keep the VMs, and the evacuated host in maintenance, for inspection
KEEP=1 ./demos/host-evacuation/run.sh

# delete the demo VMs and return hosts in maintenance to up
./demos/host-evacuation/run.sh --cleanup

# target a different API endpoint
./demos/host-evacuation/run.sh --server http://localhost:8000
```

## What it proves

1. `qarax vm create` schedules the VM onto one of the two `up` hosts and
   `qarax vm start` boots it there
2. `qarax host evacuate` puts that host into `maintenance` and blocks until the
   evacuation job has live-migrated every VM off it
3. the script then checks the VM is `running` on a different host and the
   source host is in `maintenance`
4. a second VM is created (not started: placement happens at create) and the
   script checks the scheduler put it on the other host

## Cleanup

At exit the script deletes the `demo-evac-*` VMs and returns the evacuated host
to `up`, so the shared local stack stays reusable. With `KEEP=1` it leaves both
in place; the next run (or `--cleanup`) clears them.
