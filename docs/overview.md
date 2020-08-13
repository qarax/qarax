# Overview

qarax manages hosts that run firecracker-powered VMs. The basic flow is:
1. Add a host to qarax - the host is either a physical machine or virtual (nested virtualization is required). The user provides the address and root password of the host and qarax will then configure the it:
  * Copy and launch the `qarax-node` executable
  * Download the firecracker binary
  * Configure sysctl
  * Configure networking (currently only a bridge is supported)
2. Create a VM by sending a request to the `/vms` endpoint
3. Start the VM by sending a request to the `/vms/<vm-id>/start` endpoint


## Network
If the VM requires networking, it is possible by creating the VM with:
```json
"network_mode": "dhcp"
```

Currently only DHCP mode is supported. DHCP mode will make `qarax-node` execute a simple DHCP client, which will request an IP address for MAC address generated for the VM. The VM will then start with the kernel parameter `ip=dhcp` and will get the same IP assigned to it before it was started.

To be implementd: static_ip mode, which will allow starting the VM with a predefined IP address without using DHCP.

## Storage
Currently the kernel image and the rootfs image exact location must be specified when creating the VM, but this will soon change when local and shared storage modes will be introduced, allowing better management of kernel images and VM drives.
