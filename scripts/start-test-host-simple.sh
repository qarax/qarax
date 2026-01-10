#!/bin/bash
# Simple test VM host for qarax-node development
# This version doesn't use cloud-init - just raw QEMU with SSH key setup

set -e

# Configuration
VM_NAME="${VM_NAME:-qarax-test-host}"
VM_MEMORY="${VM_MEMORY:-4096}"
VM_CPUS="${VM_CPUS:-2}"
VM_DISK_SIZE="${VM_DISK_SIZE:-20G}"
SSH_PORT="${SSH_PORT:-2222}"
ROOT_PASSWORD="${ROOT_PASSWORD:-qarax123}"

# Directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_DIR="${PROJECT_ROOT}/.vm-hosts"
VM_PATH="${VM_DIR}/${VM_NAME}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1" >&2
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1" >&2
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1" >&2
}

# Check dependencies
check_dependencies() {
    local missing=()

    for cmd in qemu-system-x86_64 qemu-img expect; do
        if ! command -v "$cmd" &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing required commands: ${missing[*]}"
        log_info "Install with:"
        log_info "  Arch: sudo pacman -S qemu-full expect"
        log_info "  Fedora/RHEL: sudo dnf install qemu-system-x86 expect"
        log_info "  Ubuntu/Debian: sudo apt install qemu-system-x86 expect"
        exit 1
    fi
}

# Generate SSH key
setup_ssh_key() {
    local ssh_key="${VM_PATH}/id_rsa"

    if [ ! -f "$ssh_key" ]; then
        log_info "Generating SSH key for VM access..." >&2
        ssh-keygen -t rsa -b 4096 -f "$ssh_key" -N "" -C "qarax-test-host" >/dev/null 2>&1
    fi

    echo "$ssh_key"
}

# Download Alpine Linux (small and fast)
download_image() {
    local image_url="https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/alpine-virt-3.19.0-x86_64.iso"
    local image_file="${VM_DIR}/alpine-virt.iso"

    if [ ! -f "$image_file" ]; then
        log_info "Downloading Alpine Linux (40MB, fast!)..." >&2
        mkdir -p "$VM_DIR"
        curl -L "$image_url" -o "$image_file" 2>&1 | grep -v "%" >&2
    else
        log_info "Using existing Alpine ISO: $image_file" >&2
    fi

    echo "$image_file"
}

# Create VM disk
create_vm_disk() {
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
    qemu-img create -f qcow2 "$vm_disk" "$VM_DISK_SIZE" >&2

    echo "$vm_disk"
}

# Create setup script that will run inside VM
create_setup_script() {
    local ssh_key="$1"
    local pub_key="${ssh_key}.pub"
    local setup_script="${VM_PATH}/setup.sh"

    log_info "Creating VM setup script..." >&2

    cat > "$setup_script" <<EOF
#!/bin/sh
# This script runs inside the VM to set it up for qarax-node

set -e

echo "=== Setting up qarax test host ==="

# Setup repositories
setup-apkrepos -1

# Update package index
apk update

# Install essential packages
apk add openssh qemu-img socat iproute2-tc iptables bridge vim htop strace tcpdump bash

# Enable and configure SSH
rc-update add sshd default
mkdir -p /root/.ssh
chmod 700 /root/.ssh
cat > /root/.ssh/authorized_keys <<'SSHKEY'
$(cat "$pub_key")
SSHKEY
chmod 600 /root/.ssh/authorized_keys

# Enable root login with key
sed -i 's/#PermitRootLogin.*/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config

# Create qarax directories
mkdir -p /var/lib/qarax/vms /var/lib/qarax/images /var/lib/qarax/disks /etc/qarax-node

# Setup qarax-node OpenRC service
cat > /etc/init.d/qarax-node <<'SERVICE'
#!/sbin/openrc-run

name="qarax-node"
description="Qarax Node - Virtual Machine Manager"
command="/usr/local/bin/qarax-node"
command_args="--port 50051 --host 0.0.0.0"
command_background="yes"
pidfile="/run/\${RC_SVCNAME}.pid"
output_log="/var/log/qarax-node.log"
error_log="/var/log/qarax-node.log"

depend() {
    need net
    after firewall
}
SERVICE

chmod +x /etc/init.d/qarax-node

# Enable IP forwarding for VM networking
echo "net.ipv4.ip_forward = 1" >> /etc/sysctl.conf
sysctl -p

# Enable required services at boot
rc-update add networking boot
rc-update add hostname boot
rc-update add sshd default

# Set hostname
echo "qarax-test-host" > /etc/hostname
hostname qarax-test-host

echo "=== Setup complete! ==="
echo "Shutting down for disk commit..."
poweroff
EOF

    chmod +x "$setup_script"
    echo "$setup_script"
}

# Create bootable setup ISO
create_setup_iso() {
    local setup_script="$1"
    local setup_iso="${VM_PATH}/setup.iso"

    log_info "Creating setup ISO..." >&2

    # Create a temporary directory for ISO contents
    local temp_dir="${VM_PATH}/iso-temp"
    mkdir -p "$temp_dir"
    cp "$setup_script" "$temp_dir/setup.sh"

    # Create ISO
    if command -v xorriso &> /dev/null; then
        xorriso -as mkisofs -output "$setup_iso" -volid "SETUP" -joliet -rock "$temp_dir" >/dev/null 2>&1
    elif command -v genisoimage &> /dev/null; then
        genisoimage -o "$setup_iso" -V "SETUP" -r -J "$temp_dir" >/dev/null 2>&1
    else
        log_error "Need xorriso or genisoimage to create ISO"
        exit 1
    fi

    rm -rf "$temp_dir"
    echo "$setup_iso"
}

# Install Alpine to disk (automated)
install_alpine() {
    local alpine_iso="$1"
    local vm_disk="$2"
    local setup_script="$3"

    log_info "Installing Alpine Linux to disk (this takes 2-3 minutes)..." >&2

    # Create expect script to automate Alpine installation
    local expect_script="${VM_PATH}/install.exp"
    cat > "$expect_script" <<'EXPECT_END'
#!/usr/bin/expect -f

set timeout 300
set iso [lindex $argv 0]
set disk [lindex $argv 1]
set setup [lindex $argv 2]

spawn qemu-system-x86_64 \
    -m 2048 \
    -smp 2 \
    -drive file=$disk,if=virtio,format=qcow2 \
    -cdrom $iso \
    -boot d \
    -nographic

# Wait for login prompt
expect {
    "localhost login:" { send "root\r" }
    timeout { exit 1 }
}

expect "localhost:~#"
send "setup-alpine -q\r"

expect "keyboard layout" { send "us\r" }
expect "variant" { send "us\r" }
expect "hostname" { send "qarax-test-host\r" }
expect "network interface" { send "eth0\r" }
expect "Ip address" { send "dhcp\r" }
expect "manual network" { send "n\r" }
expect "password" { send "qarax123\r" }
expect "password" { send "qarax123\r" }
expect "timezone" { send "UTC\r" }
expect "proxy" { send "none\r" }
expect "NTP client" { send "chrony\r" }
expect "APK mirror" { send "1\r" }

expect "SSH server" { send "openssh\r" }
expect "disk" { send "vda\r" }
expect "disk mode" { send "sys\r" }
expect "Erase" { send "y\r" }

# Wait for installation to complete
expect {
    "Installation is complete" { send "\r" }
    "localhost:~#" { send "\r" }
    timeout { exit 1 }
}

sleep 2
send "poweroff\r"
expect eof
EXPECT_END

    chmod +x "$expect_script"

    log_info "Starting automated installation..." >&2
    "$expect_script" "$alpine_iso" "$vm_disk" "$setup_script" >&2

    if [ $? -eq 0 ]; then
        log_info "Installation completed successfully!" >&2
    else
        log_error "Installation failed" >&2
        exit 1
    fi
}

# Run setup script in VM
run_setup() {
    local vm_disk="$1"
    local setup_iso="$2"

    log_info "Running setup script in VM..." >&2

    qemu-system-x86_64 \
        -m 2048 \
        -smp 2 \
        -drive file="$vm_disk",if=virtio,format=qcow2 \
        -cdrom "$setup_iso" \
        -boot c \
        -nographic \
        -kernel-kvm &

    local pid=$!

    # Wait for VM to shutdown
    wait $pid

    log_info "Setup completed!" >&2
}

# Start the VM
start_vm() {
    local vm_disk="$1"
    local pid_file="${VM_PATH}/qemu.pid"
    local monitor_socket="${VM_PATH}/qemu.monitor"
    local serial_log="${VM_PATH}/serial.log"

    # Check if already running
    if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
        log_warn "VM is already running (PID: $(cat "$pid_file"))"
        return
    fi

    log_info "Starting VM: ${VM_NAME}" >&2
    log_info "  Memory: ${VM_MEMORY}MB" >&2
    log_info "  CPUs: ${VM_CPUS}" >&2
    log_info "  SSH Port: ${SSH_PORT}" >&2

    qemu-system-x86_64 \
        -name "$VM_NAME" \
        -machine accel=kvm \
        -cpu host \
        -smp "$VM_CPUS" \
        -m "$VM_MEMORY" \
        -drive file="$vm_disk",if=virtio,format=qcow2 \
        -netdev user,id=net0,hostfwd=tcp::${SSH_PORT}-:22 \
        -device virtio-net-pci,netdev=net0 \
        -display none \
        -serial file:"$serial_log" \
        -monitor unix:"$monitor_socket",server,nowait \
        -pidfile "$pid_file" \
        -daemonize

    log_info "VM started successfully!" >&2

    # Wait for SSH to be available
    log_info "Waiting for SSH to be available..." >&2
    local max_attempts=30
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if nc -z localhost "$SSH_PORT" 2>/dev/null; then
            log_info "SSH is ready!" >&2
            return
        fi
        sleep 2
        attempt=$((attempt + 1))
        echo -n "." >&2
    done
    echo >&2

    log_warn "SSH might not be ready yet. Check manually." >&2
}

# Show connection info
show_connection_info() {
    local ssh_key="$1"

    log_info "" >&2
    log_info "===========================================" >&2
    log_info "VM Host: ${VM_NAME}" >&2
    log_info "===========================================" >&2
    log_info "" >&2
    log_info "SSH to VM:" >&2
    log_info "  ssh -i ${ssh_key} -p ${SSH_PORT} root@localhost" >&2
    log_info "" >&2
    log_info "Or use the script:" >&2
    log_info "  $0 ssh" >&2
    log_info "" >&2
    log_info "Deploy qarax-node:" >&2
    log_info "  cargo build -p qarax-node" >&2
    log_info "  scp -i ${ssh_key} -P ${SSH_PORT} target/debug/qarax-node root@localhost:/usr/local/bin/" >&2
    log_info "  ssh -i ${ssh_key} -p ${SSH_PORT} root@localhost 'rc-service qarax-node start'" >&2
    log_info "" >&2
    log_info "Or use dev-deploy script:" >&2
    log_info "  ./scripts/dev-deploy.sh" >&2
    log_info "" >&2
    log_info "Stop VM:" >&2
    log_info "  $0 stop" >&2
    log_info "" >&2
    log_info "VM files location: ${VM_PATH}" >&2
    log_info "===========================================" >&2
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

            # Check if already installed
            if [ -f "${VM_PATH}/.installed" ]; then
                log_info "VM already installed, starting..."
                ssh_key=$(setup_ssh_key)
                vm_disk="${VM_PATH}/disk.qcow2"
                start_vm "$vm_disk"
                show_connection_info "$ssh_key"
                exit 0
            fi

            log_info "First time setup - installing Alpine Linux..."

            ssh_key=$(setup_ssh_key)
            alpine_iso=$(download_image)
            vm_disk=$(create_vm_disk)
            setup_script=$(create_setup_script "$ssh_key")
            setup_iso=$(create_setup_iso "$setup_script")

            # Install Alpine
            install_alpine "$alpine_iso" "$vm_disk" "$setup_script"

            # Mark as installed
            touch "${VM_PATH}/.installed"

            log_info "Installation complete! Starting VM..."
            start_vm "$vm_disk"
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
            ssh -i "$ssh_key" -p "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@localhost "$@"
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

        logs)
            local serial_log="${VM_PATH}/serial.log"
            if [ -f "$serial_log" ]; then
                tail -f "$serial_log"
            else
                log_error "No serial log found"
                exit 1
            fi
            ;;

        *)
            echo "Usage: $0 {start|stop|status|ssh|clean|logs}"
            echo ""
            echo "Commands:"
            echo "  start   - Install (first time) and start the test VM host"
            echo "  stop    - Stop the test VM host"
            echo "  status  - Show VM status"
            echo "  ssh     - SSH into the VM as root"
            echo "  clean   - Stop and delete the VM"
            echo "  logs    - Show VM serial console logs"
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
