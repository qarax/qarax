# Firecracker Demo

Run a VM on qarax with the **Firecracker** backend and exercise its lifecycle:
create → start → pause → resume → stop.

The demo shows:

- each UP host's Firecracker version (it fails if none has Firecracker)
- `qarax vm get`, whose `Hypervisor:` line reads `firecracker`
- the guest's boot output (the kernel's `Linux version` line and `qarax-init`
  starting), read from Firecracker's log on the node, which carries the guest
  serial console. `qarax vm console` only reads Cloud Hypervisor's console log,
  so the demo reads `/var/lib/qarax/vms/<vm-id>.fc.log` in the node container.
  On a stack whose node isn't a local compose container (`make run-local VM=1`)
  this step is skipped with a warning.
- the VM's status after pause, resume and stop (the demo fails if it isn't
  `paused`, `running`, `shutdown`)

## Prerequisites

- qarax stack running (`make run-local`), or let the script start it
- `jq`
- Firecracker available on qarax-node (`/usr/local/bin/firecracker`; the local
  qarax-node image ships it)
- `qarax` CLI: built from this checkout when cargo is available, else taken
  from `PATH`. Set `SKIP_BUILD=1` to use an existing build.

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/firecracker/run.sh

# Custom API endpoint
./demos/firecracker/run.sh --server http://localhost:8000

# Keep the VM (demo-firecracker-vm) for inspection
KEEP=1 ./demos/firecracker/run.sh

# Remove what an earlier run left behind
./demos/firecracker/run.sh --cleanup
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `128` | Memory in MiB |
| `--cleanup` | | Delete the demo's VM, then exit |

## Cleanup

Each run first deletes VMs left by an earlier run (names start with
`demo-firecracker-`), and deletes its VM on exit, including when a step
fails, unless `KEEP=1` is set.
