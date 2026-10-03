# Host Maintenance / Evacuation Demo

Demonstrates Qarax's manual host evacuation workflow on a live two-host stack:

- verify two hosts are `up`
- create and start a small live-migration-compatible VM
- identify the VM's source host
- evacuate that host
- show the host ends in `maintenance`
- show the VM moved to the other host
- create another VM and show new placement avoids the maintenance host

## Prerequisites

- a live two-node Qarax stack with both hosts `up`
- `jq`
- `qarax` CLI built in `target/` or on PATH, or a Rust toolchain so the demo can build it

If your server uses a token other than the local default, export `QARAX_TOKEN`.

The simplest two-node setup is `make run-local`, which already starts a second
node container (`qarax-node-2`) but only registers the first one. Register the
second node:

```bash
make run-local
QARAX_TOKEN=${QARAX_TOKEN:-e2e-test-token} \
  bash e2e/setup_host.sh http://localhost:8000 qarax-node-2 50051 local-node-2
```

Alternatively, keep the two-node e2e stack running after its evacuation test:

```bash
cd e2e
KEEP=1 ./run_e2e_tests.sh test_live_migration.py::test_host_evacuation_marks_maintenance_and_avoids_rescheduling
```

## Usage

```bash
./demos/host-evacuation/run.sh

# target a different API endpoint
./demos/host-evacuation/run.sh --server http://localhost:8000
```

## What it proves

The demo walks the real operator workflow:

1. schedule a VM onto one of two `up` hosts
2. evacuate that source host with `qarax host evacuate`
3. confirm the host stays manageable but ends in `maintenance`
4. confirm the running VM has moved away
5. create a second VM and confirm the scheduler excludes the maintenance host

## Cleanup

The script cleans up both demo VMs automatically and returns the evacuated host to
`up` on exit so the shared local stack is left reusable.
