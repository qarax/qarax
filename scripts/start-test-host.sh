#!/bin/bash
# Start a test VM host for qarax-node development
# This creates a VM that can be used as a target for qarax-node deployment

set -e

# Configuration
VM_NAME="${VM_NAME:-qarax-test-host}"
VM_MEMORY="${VM_MEMORY:-4096}"  # 4GB
VM_CPUS="${VM_CPUS:-2}"
VM_DISK_SIZE="${VM_DISK_SIZE:-20G}"
VM_IMAGE_URL="${VM_IMAGE_URL:-https://download.fedoraproject.org/pub/fedora/linux/releases/39/Cloud/x86_64/images/Fedora-Cloud-Base-39-1.5.x86_64.qcow2}"
VM_USER="${VM_USER:-fedora}"
SSH_PORT="${SSH_PORT:-2222}"

# Directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_DIR="${PROJECT_ROOT}/.vm-hosts"
VM_PATH="${VM_DIR}/${VM_NAME}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check dependencies
check_dependencies() {
    local missing=()

    for cmd in qemu-system-x86_64 qemu-img xorriso; do
        if ! command -v "$cmd" &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing required commands: ${missing[*]}"
        log_info "Install with:"
        log_info "  Arch: sudo pacman -S qemu-full xorriso"
        log_info "  Fedora/RHEL: sudo dnf install qemu-system-x86 cloud-utils xorriso"
        log_info "  Ubuntu/Debian: sudo apt install qemu-system-x86 cloud-image-utils xorriso"
        exit 1
    fi
}

# Generate SSH key if it doesn't exist
setup_ssh_key() {
    local ssh_key="${VM_PATH}/id_rsa"

    if [ ! -f "$ssh_key" ]; then
        log_info "Generating SSH key for VM access..." >&2
        ssh-keygen -t rsa -b 4096 -f "$ssh_key" -N "" -C "qarax-test-host" >/dev/null 2>&1
    fi

    echo "$ssh_key"
}

# Download cloud image if needed
download_image() {
    local image_name=$(basename "$VM_IMAGE_URL")
    local image_file="${VM_DIR}/${image_name}"

    if [ ! -f "$image_file" ]; then
        log_info "Downloading cloud image..." >&2
        mkdir -p "$VM_DIR"
        curl -L "$VM_IMAGE_URL" -o "$image_file" 2>&1 | grep -v "%" >&2
    else
        log_info "Using existing cloud image: $image_file" >&2
    fi

    echo "$image_file"
}

# Create VM disk from cloud image
create_vm_disk() {
    local base_image="$1"
    local vm_disk="${VM_PATH}/disk.qcow2"

    if [ -f "$vm_disk" ]; then
        log_warn "VM disk already exists: $vm_disk" >&2
        read -p "Delete and recreate? (y/N): " -n 1 -r >&2
        echo >&2
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "Using existing disk" >&2
            echo "$vm_disk"
            return
        fi
        rm -f "$vm_disk"
    fi

    log_info "Creating VM disk (${VM_DISK_SIZE})..." >&2
    qemu-img create -f qcow2 -F qcow2 -b "$base_image" "$vm_disk" "$VM_DISK_SIZE" >&2

    echo "$vm_disk"
}

# Create cloud-init configuration
create_cloud_init() {
    local ssh_key="$1"
    local pub_key="${ssh_key}.pub"
    local cloud_init_iso="${VM_PATH}/cloud-init.iso"

    log_info "Creating cloud-init configuration..." >&2

    # Create meta-data
    cat > "${VM_PATH}/meta-data" <<EOF
instance-id: ${VM_NAME}
local-hostname: ${VM_NAME}
EOF

    # Create user-data with SSH key and qarax-node setup
    cat > "${VM_PATH}/user-data" <<EOF
#cloud-config
users:
  - name: ${VM_USER}
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - $(cat "$pub_key")
  - name: root
    ssh_authorized_keys:
      - $(cat "$pub_key")

# Update system and install dependencies
package_update: true
package_upgrade: true

packages:
  - qemu-img
  - socat
  - iproute-tc
  - iptables
  - bridge-utils
  - vim
  - htop
  - strace
  - tcpdump

# Enable services
runcmd:
  # Create qarax directories
  - mkdir -p /var/lib/qarax/vms /var/lib/qarax/images /var/lib/qarax/disks /etc/qarax-node

  # Setup qarax-node systemd service (will be populated by deployment)
  - |
    cat > /etc/systemd/system/qarax-node.service <<'EOFS'
    [Unit]
    Description=Qarax Node - Virtual Machine Manager
    After=network-online.target
    Wants=network-online.target

    [Service]
    Type=simple
    ExecStart=/usr/local/bin/qarax-node --port 50051 --host 0.0.0.0
    Restart=always
    RestartSec=5
    WorkingDirectory=/var/lib/qarax
    Environment="RUST_LOG=info"
    Environment="RUST_BACKTRACE=1"
    StandardOutput=journal
    StandardError=journal

    [Install]
    WantedBy=multi-user.target
    EOFS

  # Enable qarax-node (won't start until binary is deployed)
  - systemctl daemon-reload
  - systemctl enable qarax-node

  # Configure networking for VMs
  - echo "net.ipv4.ip_forward = 1" >> /etc/sysctl.conf
  - sysctl -p

  # Allow SSH for root (for deployment)
  - sed -i 's/^#PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
  - systemctl restart sshd

write_files:
  - path: /etc/motd
    content: |
      ================================================
      Qarax Test Host: ${VM_NAME}

      This VM is configured as a qarax-node test host.

      To deploy qarax-node:
        cargo build -p qarax-node
        scp target/debug/qarax-node root@localhost:2222:/usr/local/bin/
        ssh root@localhost -p 2222 "systemctl restart qarax-node"

      SSH as root: ssh root@localhost -p ${SSH_PORT}
      SSH as user:  ssh ${VM_USER}@localhost -p ${SSH_PORT}
      ================================================

power_state:
  mode: reboot
  timeout: 60
  condition: true
EOF

    # Create cloud-init ISO using xorriso (compatible with all distros)
    local temp_dir="${VM_PATH}/cloud-init-files"
    mkdir -p "$temp_dir"
    cp "${VM_PATH}/user-data" "$temp_dir/user-data"
    cp "${VM_PATH}/meta-data" "$temp_dir/meta-data"

    xorriso -as mkisofs \
        -output "$cloud_init_iso" \
        -volid cidata \
        -joliet \
        -rock \
        "$temp_dir" > /dev/null 2>&1

    rm -rf "$temp_dir"

    echo "$cloud_init_iso"
}

# Start the VM
start_vm() {
    local vm_disk="$1"
    local cloud_init_iso="$2"
    local pid_file="${VM_PATH}/qemu.pid"
    local monitor_socket="${VM_PATH}/qemu.monitor"

    # Check if already running
    if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
        log_warn "VM is already running (PID: $(cat "$pid_file"))"
        return
    fi

    log_info "Starting VM: ${VM_NAME}"
    log_info "  Memory: ${VM_MEMORY}MB"
    log_info "  CPUs: ${VM_CPUS}"
    log_info "  SSH Port: ${SSH_PORT}"

    qemu-system-x86_64 \
        -name "$VM_NAME" \
        -machine accel=kvm \
        -cpu host \
        -smp "$VM_CPUS" \
        -m "$VM_MEMORY" \
        -drive file="$vm_disk",if=virtio,format=qcow2 \
        -drive file="$cloud_init_iso",if=virtio,format=raw \
        -netdev user,id=net0,hostfwd=tcp::${SSH_PORT}-:22 \
        -device virtio-net-pci,netdev=net0 \
        -display none \
        -monitor unix:"$monitor_socket",server,nowait \
        -pidfile "$pid_file" \
        -daemonize

    log_info "VM started successfully!"
    log_info ""
    log_info "Waiting for VM to boot (this may take 30-60 seconds)..."

    # Wait for SSH to be available
    local max_attempts=60
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if nc -z localhost "$SSH_PORT" 2>/dev/null; then
            log_info "VM is ready!"
            break
        fi
        sleep 2
        attempt=$((attempt + 1))
        echo -n "."
    done
    echo ""

    if [ $attempt -eq $max_attempts ]; then
        log_warn "VM might still be booting. Check manually."
    fi
}

# Show connection info
show_connection_info() {
    local ssh_key="$1"

    log_info ""
    log_info "==========================================="
    log_info "VM Host: ${VM_NAME}"
    log_info "==========================================="
    log_info ""
    log_info "SSH as root:"
    log_info "  ssh -i ${ssh_key} -p ${SSH_PORT} root@localhost"
    log_info ""
    log_info "SSH as ${VM_USER}:"
    log_info "  ssh -i ${ssh_key} -p ${SSH_PORT} ${VM_USER}@localhost"
    log_info ""
    log_info "Deploy qarax-node:"
    log_info "  cargo build -p qarax-node"
    log_info "  scp -i ${ssh_key} -P ${SSH_PORT} target/debug/qarax-node root@localhost:/usr/local/bin/"
    log_info "  ssh -i ${ssh_key} -p ${SSH_PORT} root@localhost 'systemctl restart qarax-node'"
    log_info ""
    log_info "Stop VM:"
    log_info "  $0 stop"
    log_info ""
    log_info "VM files location: ${VM_PATH}"
    log_info "==========================================="
}

# Stop the VM
stop_vm() {
    local pid_file="${VM_PATH}/qemu.pid"

    if [ ! -f "$pid_file" ]; then
        log_warn "VM is not running (no PID file found)"
        return
    fi

    local pid=$(cat "$pid_file")
    if ! kill -0 "$pid" 2>/dev/null; then
        log_warn "VM is not running (stale PID file)"
        rm -f "$pid_file"
        return
    fi

    log_info "Stopping VM (PID: $pid)..."
    kill "$pid"

    # Wait for process to exit
    local timeout=10
    while [ $timeout -gt 0 ] && kill -0 "$pid" 2>/dev/null; do
        sleep 1
        timeout=$((timeout - 1))
    done

    if kill -0 "$pid" 2>/dev/null; then
        log_warn "VM didn't stop gracefully, forcing..."
        kill -9 "$pid"
    fi

    rm -f "$pid_file"
    log_info "VM stopped"
}

# Show VM status
show_status() {
    local pid_file="${VM_PATH}/qemu.pid"

    if [ ! -f "$pid_file" ]; then
        echo "VM Status: NOT RUNNING"
        return
    fi

    local pid=$(cat "$pid_file")
    if kill -0 "$pid" 2>/dev/null; then
        echo "VM Status: RUNNING (PID: $pid)"
        echo "SSH Port: $SSH_PORT"
        echo "VM Directory: $VM_PATH"
    else
        echo "VM Status: NOT RUNNING (stale PID file)"
    fi
}

# Main script
main() {
    local command="${1:-start}"

    case "$command" in
        start)
            check_dependencies
            mkdir -p "$VM_PATH"

            ssh_key=$(setup_ssh_key)
            base_image=$(download_image)
            vm_disk=$(create_vm_disk "$base_image")
            cloud_init=$(create_cloud_init "$ssh_key")

            start_vm "$vm_disk" "$cloud_init"
            show_connection_info "$ssh_key"
            ;;

        stop)
            stop_vm
            ;;

        status)
            show_status
            ;;

        ssh)
            local ssh_key="${VM_PATH}/id_rsa"
            if [ ! -f "$ssh_key" ]; then
                log_error "SSH key not found. Is the VM created?"
                exit 1
            fi
            shift
            ssh -i "$ssh_key" -p "$SSH_PORT" root@localhost "$@"
            ;;

        clean)
            stop_vm
            log_warn "This will DELETE the VM and all its data!"
            read -p "Are you sure? (y/N): " -n 1 -r
            echo
            if [[ $REPLY =~ ^[Yy]$ ]]; then
                rm -rf "$VM_PATH"
                log_info "VM cleaned"
            fi
            ;;

        *)
            echo "Usage: $0 {start|stop|status|ssh|clean}"
            echo ""
            echo "Commands:"
            echo "  start   - Start the test VM host"
            echo "  stop    - Stop the test VM host"
            echo "  status  - Show VM status"
            echo "  ssh     - SSH into the VM as root"
            echo "  clean   - Stop and delete the VM"
            echo ""
            echo "Environment variables:"
            echo "  VM_NAME       - VM name (default: qarax-test-host)"
            echo "  VM_MEMORY     - Memory in MB (default: 4096)"
            echo "  VM_CPUS       - Number of CPUs (default: 2)"
            echo "  VM_DISK_SIZE  - Disk size (default: 20G)"
            echo "  SSH_PORT      - SSH port on host (default: 2222)"
            exit 1
            ;;
    esac
}

main "$@"
