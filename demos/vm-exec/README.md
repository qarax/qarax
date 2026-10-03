# VM Exec Demo

This demo shows the new regular-VM guest exec flow end-to-end:

1. Create a local storage pool
2. Transfer the built-in demo kernel and guest-agent initramfs
3. Create a boot source and Cloud Hypervisor VM template
4. Create a VM from that template with `--guest-agent`
5. Start the VM
6. Run `qarax vm exec` inside the guest

It reuses the same built-in artifacts that already work in the repo's local/e2e stack:

- kernel: `/var/lib/qarax/images/vmlinux`
- initramfs: `/var/lib/qarax/images/test-initramfs.gz`

The initramfs includes `qarax-init`, so the guest agent is available for `qarax vm exec`.

## Prerequisites

- qarax stack running: `./hack/run-local.sh` (the script starts it if it is not up)
- `jq`
- `qarax` CLI built with guest exec support (built automatically if missing)

If your server uses a token other than the local default, export `QARAX_TOKEN`.

## Usage

```bash
./demos/vm-exec/run.sh
```

Optional flags:

- `--server URL` — override the API endpoint
- `--host NAME` — attach the demo pool to a specific host
- `--keep` — keep the VM/template/pool for inspection instead of cleaning up

## What success looks like

The demo prints the `qarax vm exec` output from inside the guest. The command
is `printf vm-exec && uname -s`, so the expected stdout is the marker followed
directly by the kernel name:

```text
vm-execLinux
```
