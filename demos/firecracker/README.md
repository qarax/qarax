# Firecracker Demo

Run a VM on Qarax using the **Firecracker** backend and exercise the full lifecycle:
create → start → pause → resume → stop → delete.

## Prerequisites

- qarax stack running (`make run-local`) or let the script auto-start it
- Rust toolchain: the script runs `cargo build -p cli` first. Set `SKIP_BUILD=1`
  to use an existing `qarax` binary (under `target/` or on PATH) instead.
- Firecracker available on qarax-node (`/usr/local/bin/firecracker`; the local
  qarax-node image ships it). The script warns if no `up` host reports a
  Firecracker version.
- If your server uses a token other than the local default, export `QARAX_TOKEN`

## Usage

```bash
./demos/firecracker/run.sh

# Custom API endpoint
./demos/firecracker/run.sh --server http://localhost:8000

# Keep VM for inspection
./demos/firecracker/run.sh --no-cleanup

# Skip the CLI build
SKIP_BUILD=1 ./demos/firecracker/run.sh
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |
| `--name NAME` | `fc-demo-<timestamp>-<pid>` | VM name |
| `--vcpus N` | `1` | vCPU count |
| `--memory MiB` | `128` | Memory in MiB |
| `--no-cleanup` | off | Leave VM after demo (also on failure) |

## Notes

- This demo explicitly uses `--hypervisor firecracker`.
- It waits for each lifecycle state transition before continuing and prints the
  VM details if a transition times out.
- Unless `--no-cleanup` is set, the VM is force-stopped and deleted on exit,
  including when a step fails.
