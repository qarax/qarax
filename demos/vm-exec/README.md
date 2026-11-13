# VM Exec Demo

Runs a command inside a VM with `qarax vm exec`:

1. Create a VM with `--guest-agent`
2. Start it and wait for the guest agent to answer
3. Run `qarax vm exec` inside the guest and check its output

No boot source or template is needed. On the local stack a VM created without
one boots the default kernel (`/var/lib/qarax/images/vmlinux`) and initramfs
(`/var/lib/qarax/images/test-initramfs.gz`). The initramfs runs `qarax-init`
as PID 1, which serves guest exec on vsock port 7000; `--guest-agent` gives
the VM the vsock device qarax-node connects through.

## Prerequisites

- qarax stack running: `make run-local` (the script starts it via `./hack/run-local.sh` if it is not up)
- `jq`
- `qarax` CLI: built from this checkout when cargo is available, else taken from `PATH`

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/vm-exec/run.sh

# Custom API endpoint
./demos/vm-exec/run.sh --server http://localhost:8000

# Leave the VM (demo-vm-exec-vm) running to try `qarax vm exec` yourself
KEEP=1 ./demos/vm-exec/run.sh

# Remove what an earlier run left behind
./demos/vm-exec/run.sh --cleanup
```

Each run first deletes VMs left by an earlier run (names start with
`demo-vm-exec-`).

## What success looks like

The demo prints the `qarax vm exec` output from inside the guest. The command
is `printf vm-exec && uname -s`, so the expected stdout is the marker followed
directly by the kernel name:

```text
vm-execLinux
```

The demo fails if the guest agent doesn't answer within 60 seconds or the
output differs.
