# qarax Development Scripts

This directory contains helper scripts for qarax development and testing.

## Scripts Overview

### VM Creation Scripts (Choose One)

We provide three options for creating test VMs, from simplest to most automated:

#### 1. `start-test-host-manual.sh` ⭐ RECOMMENDED

**Simplest option** - Manual Alpine Linux setup with minimal dependencies.

**Quick Start:**
```bash
# Step 1: Install Alpine (follow prompts)
./scripts/start-test-host-manual.sh install

# Step 2: Configure SSH (follow prompts)
./scripts/start-test-host-manual.sh setup

# Step 3: Start the VM
./scripts/start-test-host-manual.sh start

# Step 4: SSH into it
./scripts/start-test-host-manual.sh ssh
```

**Requirements:** Only `qemu-system-x86_64` and `qemu-img`
```bash
sudo pacman -S qemu-full  # Arch
```

**Pros:**
- Minimal dependencies
- Small download (40MB Alpine ISO)
- Full control over setup
- Works everywhere

**Cons:**
- Manual installation steps
- Takes 5-10 minutes for initial setup

---

#### 2. `start-test-host-simple.sh`

Fully automated Alpine installation using `expect`.

**Quick Start:**
```bash
./scripts/start-test-host-simple.sh start
```

**Requirements:** qemu + expect
```bash
sudo pacman -S qemu-full expect  # Arch
```

**Pros:**
- Fully automated
- Small download (40MB)

**Cons:**
- Requires `expect` package
- Less control over setup

---

#### 3. `start-test-host.sh`

Cloud-init based with Fedora Cloud image.

**Quick Start:**
```bash
./scripts/start-test-host.sh start
```

**Requirements:** qemu + xorriso (or cloud-utils)
```bash
sudo pacman -S qemu-full xorriso  # Arch
```

**Pros:**
- Standard cloud-init approach
- Well-tested Fedora base

**Cons:**
- Large download (500MB+ Fedora image)
- Slower initial setup

---

### Common VM Commands

All three scripts support the same basic commands:

```bash
# Start the VM
./scripts/start-test-host-*.sh start

# SSH into the VM
./scripts/start-test-host-*.sh ssh

# Stop the VM
./scripts/start-test-host-*.sh stop

# Show status
./scripts/start-test-host-*.sh status

# Clean up everything
./scripts/start-test-host-*.sh clean
```

### `dev-deploy.sh`

Quick deployment script for qarax-node during development.

**Usage:**
```bash
# Build and deploy qarax-node to test host
./scripts/dev-deploy.sh

# Deploy and watch logs
./scripts/dev-deploy.sh --logs

# Deploy release build
./scripts/dev-deploy.sh --release
```

**Features:**
- Auto-builds qarax-node
- Deploys via SCP
- Restarts qarax-node service
- Can watch logs after deployment

---

## Development Workflows

### Quick Development Cycle

1. **Start test host:**
   ```bash
   ./scripts/start-test-host-manual.sh start
   ```

2. **Build and deploy qarax-node:**
   ```bash
   # Option 1: Use dev-deploy script
   ./scripts/dev-deploy.sh
   
   # Option 2: Manual deployment
   cargo build -p qarax-node
   scp -P 2222 target/debug/qarax-node root@localhost:/usr/local/bin/
   ./scripts/start-test-host-manual.sh ssh "/usr/local/bin/qarax-node --version"
   ```

3. **Watch logs:**
   ```bash
   ./scripts/start-test-host-manual.sh logs
   ```

### Automated Development Loop

```bash
# Terminal 1: Auto-rebuild and deploy
cargo watch -x 'build -p qarax-node' -s './scripts/dev-deploy.sh'

# Terminal 2: Watch logs
./scripts/start-test-host-manual.sh logs

# Terminal 3: Run qarax control plane
cd qarax && cargo run
```

### Testing VM Creation on Test Host

Once qarax-node is deployed to the test host, test it:

```bash
# Check qarax-node is working
./scripts/start-test-host-manual.sh ssh "/usr/local/bin/qarax-node --version"

# Check if qarax-node can communicate
# (qarax control plane needs to be running)
curl http://localhost:8000/hosts
```

### Multi-Host Testing

Run multiple test hosts on different ports:

```bash
# Host 1
VM_NAME=host1 SSH_PORT=2222 ./scripts/start-test-host-manual.sh start

# Host 2  
VM_NAME=host2 SSH_PORT=2223 ./scripts/start-test-host-manual.sh start

# Host 3
VM_NAME=host3 SSH_PORT=2224 ./scripts/start-test-host-manual.sh start
```

## VM Host Details

### Default Configuration

- **OS:** Alpine Linux (manual/simple) or Fedora Cloud (cloud-init)
- **Memory:** 4GB (customizable with VM_MEMORY)
- **CPUs:** 2 (customizable with VM_CPUS)
- **Disk:** 10-20GB depending on script
- **SSH Port:** 2222 (customizable with SSH_PORT)
- **Users:** root (SSH key authentication)

### Pre-installed Packages

- qemu-img (disk management)
- socat, iproute2-tc (networking)
- iptables (firewall)
- bash, vim (shell and editor)

### Directory Structure

```
.vm-hosts/
├── qarax-test-host/
│   ├── id_rsa           # SSH private key
│   ├── id_rsa.pub       # SSH public key
│   ├── disk.qcow2       # VM disk
│   ├── cloud-init.iso   # Cloud-init config
│   ├── meta-data        # Cloud-init metadata
│   ├── user-data        # Cloud-init user data
│   ├── qemu.pid         # QEMU process ID
│   └── qemu.monitor     # QEMU monitor socket
└── fedora-cloud-base.qcow2  # Shared base image
```

### VM Host Layout

Inside the test VM:

```
/usr/local/bin/qarax-node         # qarax-node binary (you deploy this)
/var/lib/qarax/                   # qarax working directory
  ├── vms/                        # VM runtime data
  ├── images/                     # VM images
  └── disks/                      # VM disks
```

**Note:** The VM does NOT have qarax-node pre-installed. You must deploy it yourself using `dev-deploy.sh` or manual SCP.

## Troubleshooting

### VM Won't Start

**KVM not available:**
```bash
# Check if KVM is available
ls -l /dev/kvm

# Enable KVM on Intel
sudo modprobe kvm_intel

# Enable KVM on AMD
sudo modprobe kvm_amd
```

**Port already in use:**
```bash
# Use different SSH port
SSH_PORT=3333 ./scripts/start-test-host-manual.sh start
```

### Can't SSH to VM

**Wait for boot:**
The VM takes 30-60 seconds to fully boot. Wait and try again.

**Check VM is running:**
```bash
./scripts/start-test-host-manual.sh status
```

**Manual SSH:**
```bash
ssh -i .vm-hosts/qarax-test-host/id_rsa -p 2222 \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  root@localhost
```

### qarax-node Won't Start

**Binary not deployed:**
```bash
./scripts/start-test-host-manual.sh ssh "ls -l /usr/local/bin/qarax-node"
```

**Check if qarax-node runs:**
```bash
./scripts/start-test-host-manual.sh ssh "/usr/local/bin/qarax-node --help"
```

**Deploy qarax-node:**
```bash
./scripts/dev-deploy.sh
```

### Clean Start

If things are broken, clean everything and start fresh:

```bash
./scripts/start-test-host-manual.sh clean
./scripts/start-test-host-manual.sh install
./scripts/start-test-host-manual.sh setup
./scripts/start-test-host-manual.sh start
```

## Tips

### Speed Up Deployments

Use `rsync` instead of `scp` for faster transfers:
```bash
rsync -avz -e "ssh -p 2222" target/debug/qarax-node root@localhost:/usr/local/bin/
```

### Persistent SSH Config

Add to `~/.ssh/config`:
```
Host qarax-test
    HostName localhost
    Port 2222
    User root
    IdentityFile /path/to/qarax/.vm-hosts/qarax-test-host/id_rsa
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
```

Then just:
```bash
ssh qarax-test
```

### View Serial Console Logs

Check what's happening during boot:
```bash
./scripts/start-test-host-manual.sh logs
```

### Which Script Should I Use?

- **Just want to test quickly?** → Use `start-test-host-manual.sh` (recommended)
- **Want fully automated setup?** → Use `start-test-host-simple.sh` (requires expect)
- **Need standard cloud-init?** → Use `start-test-host.sh` (larger download)