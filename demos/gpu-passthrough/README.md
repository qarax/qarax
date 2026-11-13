# GPU Passthrough Demo

Boot a VM from an OCI image (e.g. NVIDIA CUDA) with one or more GPUs passed through via VFIO.

The script lists the GPUs on a host, creates a VM with `--image-ref` and GPU scheduling
flags, starts it, and checks that the VM holds the requested number of GPUs on the host
the scheduler picked (`qarax host gpus`).

The VM (`demo-gpu-vm`) is deleted when the script exits, which releases its GPUs. Set
`KEEP=1` to leave it running. A VM left by an interrupted or `KEEP=1` run is deleted
when the script starts.

## Prerequisites

- qarax stack running: `make run-local` (the script does not start it)
- `cargo` (the script builds the `qarax` CLI) or a `qarax` CLI on PATH
- `jq`
- Host with GPU(s) bound to `vfio-pci`
- IOMMU enabled in kernel cmdline: `intel_iommu=on iommu=pt`
- If your server uses a token other than the local default, export `QARAX_TOKEN`

### Binding a GPU to vfio-pci

```bash
# Find PCI address and current driver
lspci -nnk | grep -A3 -i nvidia

# Unbind from current driver and bind to vfio-pci
sudo modprobe vfio-pci
echo 0000:01:00.0 | sudo tee /sys/bus/pci/drivers/<current_driver>/unbind
echo <VENDOR_ID> <DEVICE_ID> | sudo tee /sys/bus/pci/drivers/vfio-pci/new_id

# Verify
ls -la /dev/vfio/
```

## Usage

```bash
# Default: NVIDIA CUDA image, 1 GPU
./demos/gpu-passthrough/run.sh

# Request 2 GPUs
./demos/gpu-passthrough/run.sh --gpu-count 2

# Filter by vendor
./demos/gpu-passthrough/run.sh --gpu-vendor nvidia

# Custom CUDA image
./demos/gpu-passthrough/run.sh --image-ref nvcr.io/nvidia/cuda:12.6.3-devel-ubuntu24.04

# Leave the VM running (it keeps its GPUs until deleted)
KEEP=1 ./demos/gpu-passthrough/run.sh
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--image-ref REF` | `docker.io/nvidia/cuda:12.6.3-base-ubuntu24.04` | OCI image |
| `--gpu-count N` | `1` | Number of GPUs to request |
| `--gpu-vendor VENDOR` | — | Filter by vendor (nvidia, amd) |
| `--gpu-model MODEL` | — | Filter by model name |
| `--min-vram BYTES` | — | Minimum VRAM |
| `--vcpus N` | `4` | vCPU count |
| `--memory MiB` | `4096` | Memory in MiB |
| `--host NAME` | first host that is up | Host to list GPUs on (scheduling may still pick another GPU host) |
| `--server URL` | `$QARAX_SERVER` or `http://localhost:8000` | qarax API URL |

## Cleanup

Without `KEEP=1` there is nothing to clean up. After a `KEEP=1` run:

```bash
qarax vm delete demo-gpu-vm   # releases the GPUs
```
