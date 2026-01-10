#!/bin/bash
# Ultra-simple test VM host for qarax-node development
# Uses Alpine Linux with manual SSH setup - no cloud-init, no expect needed

set -e

# Configuration
VM_NAME="${VM_NAME:-qarax-test-host}"
VM_MEMORY="${VM_MEMORY:-4096}"
VM_CPUS="${VM_CPUS:-2}"
SSH_PORT="${SSH_PORT:-2222}"

# Directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_DIR="${PROJECT_ROOT}/.vm-hosts"
VM_PATH="${VM_DIR}/${VM_NAME}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_dependencies() {
    local missing=()
    for cmd in qemu-system-x86_64 qemu-img; do
        if ! command -v "$cmd" &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing: ${missing[*]}"
        log_info "Install: sudo pacman -S qemu-full"
        exit 1
    fi
}

setup_ssh_key() {
    local ssh_key="${VM_PATH}/id_rsa"
    if [ ! -f "$ssh_key" ]; then
        log_info "Generating SSH key..."
        ssh-keygen -t rsa -b 2048 -f "$ssh_key" -N "" -C "qarax" >/dev/null 2>&1
    fi
    echo "$ssh_key"
}

create_disk() {
    local disk="${VM_PATH}/disk.qcow2"

    if [ -f "$disk" ]; then
        log_info "Using existing disk"
        echo "$disk"
        return
    fi

    log_info "Creating disk (10G)..."
    qemu-img create -f qcow2 "$disk" 10G >/dev/null 2>&1
    echo "$disk"
}

start_vm_installer() {
    local disk="$1"
    local iso="${VM_DIR}/alpine.iso"

    # Download Alpine if needed (40MB)
    if [ ! -f "$iso" ]; then
        log_info "Downloading Alpine Linux (40MB)..."
        curl -L "https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/alpine-virt-3.19.0-x86_64.iso" \
            -o "$iso" --progress-bar
    fi

    log_info ""
    log_info "========================================"
    log_info "MANUAL SETUP REQUIRED"
    log_info "========================================"
    log_info ""
    log_info "The VM will start. Follow these steps:"
    log_info ""
    log_info "1. Login as: root (no password)"
    log_info "2. Run: setup-alpine"
    log_info "3. Settings:"
    log_info "   - Keyboard: us / us"
    log_info "   - Hostname: qarax"
    log_info "   - Interface: eth0"
    log_info "   - IP: dhcp"
    log_info "   - Manual network config: no"
    log_info "   - Root password: qarax"
    log_info "   - Timezone: UTC"
    log_info "   - Proxy: none"
    log_info "   - NTP: chrony"
    log_info "   - APK mirror: 1 (or f for fastest)"
    log_info "   - SSH: openssh"
    log_info "   - Disk: vda"
    log_info "   - Mode: sys"
    log_info "   - Erase disk: y"
    log_info ""
    log_info "4. After install completes, run: poweroff"
    log_info "5. Then run: $0 setup"
    log_info ""
    log_info "Press Enter to start VM..."
    read

    qemu-system-x86_64 \
        -m 2048 \
        -smp 2 \
        -drive file="$disk",if=virtio,format=qcow2 \
        -cdrom "$iso" \
        -boot d \
        -nographic
}

configure_ssh() {
    local disk="$1"
    local ssh_key="$2"

    log_info "Configuring SSH access..."
    log_info ""
    log_info "========================================"
    log_info "FINAL SETUP"
    log_info "========================================"
    log_info ""
    log_info "VM will boot. Login and run these commands:"
    log_info ""
    log_info "# Create SSH directory"
    log_info "mkdir -p /root/.ssh"
    log_info "chmod 700 /root/.ssh"
    log_info ""
    log_info "# Add SSH key (copy this):"
    cat "${ssh_key}.pub"
    log_info ""
    log_info "echo 'PASTE_KEY_HERE' > /root/.ssh/authorized_keys"
    log_info "chmod 600 /root/.ssh/authorized_keys"
    log_info ""
    log_info "# Install qarax dependencies"
    log_info "apk add qemu-img socat iproute2-tc iptables bash vim"
    log_info ""
    log_info "# Enable IP forwarding"
    log_info "echo 'net.ipv4.ip_forward = 1' >> /etc/sysctl.conf"
    log_info "sysctl -p"
    log_info ""
    log_info "# Create qarax directories"
    log_info "mkdir -p /var/lib/qarax /usr/local/bin"
    log_info ""
    log_info "# Shutdown when done"
    log_info "poweroff"
    log_info ""
    log_info "Press Enter to start VM..."
    read

    qemu-system-x86_64 \
        -m 2048 \
        -smp 2 \
        -drive file="$disk",if=virtio,format=qcow2 \
        -netdev user,id=net0 \
        -device virtio-net-pci,netdev=net0 \
        -nographic
}

start_vm() {
    local disk="$1"
    local pid_file="${VM_PATH}/qemu.pid"
    local monitor="${VM_PATH}/qemu.monitor"
    local log_file="${VM_PATH}/serial.log"

    if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
        log_warn "VM already running (PID: $(cat "$pid_file"))"
        return
    fi

    log_info "Starting VM..."
    log_info "  Memory: ${VM_MEMORY}MB"
    log_info "  CPUs: ${VM_CPUS}"
    log_info "  SSH: localhost:${SSH_PORT}"

    qemu-system-x86_64 \
        -name "$VM_NAME" \
        -machine accel=kvm \
        -cpu host \
        -smp "$VM_CPUS" \
        -m "$VM_MEMORY" \
        -drive file="$disk",if=virtio,format=qcow2 \
        -netdev user,id=net0,hostfwd=tcp::${SSH_PORT}-:22 \
        -device virtio-net-pci,netdev=net0 \
        -display none \
        -serial file:"$log_file" \
        -monitor unix:"$monitor",server,nowait \
        -pidfile "$pid_file" \
        -daemonize

    log_info "VM started!"
    log_info ""
    log_info "Wait 10 seconds for boot, then SSH with:"
    log_info "  ssh -i ${VM_PATH}/id_rsa -p ${SSH_PORT} root@localhost"
    log_info ""
    log_info "Or use: $0 ssh"
}

stop_vm() {
    local pid_file="${VM_PATH}/qemu.pid"

    if [ ! -f "$pid_file" ]; then
        log_warn "VM not running"
        return
    fi

    local pid=$(cat "$pid_file")
    if ! kill -0 "$pid" 2>/dev/null; then
        log_warn "VM not running (stale PID)"
        rm -f "$pid_file"
        return
    fi

    log_info "Stopping VM (PID: $pid)..."
    kill "$pid"
    sleep 2

    if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid"
    fi

    rm -f "$pid_file"
    log_info "VM stopped"
}

show_status() {
    local pid_file="${VM_PATH}/qemu.pid"

    if [ ! -f "$pid_file" ]; then
        echo "Status: NOT RUNNING"
        return
    fi

    local pid=$(cat "$pid_file")
    if kill -0 "$pid" 2>/dev/null; then
        echo "Status: RUNNING (PID: $pid)"
        echo "SSH: ssh -i ${VM_PATH}/id_rsa -p ${SSH_PORT} root@localhost"
    else
        echo "Status: NOT RUNNING"
    fi
}

main() {
    local cmd="${1:-start}"

    case "$cmd" in
        install)
            check_dependencies
            mkdir -p "$VM_PATH"
            setup_ssh_key
            disk=$(create_disk)
            start_vm_installer "$disk"
            log_info "Next: $0 setup"
            ;;

        setup)
            ssh_key=$(setup_ssh_key)
            disk="${VM_PATH}/disk.qcow2"
            configure_ssh "$disk" "$ssh_key"
            touch "${VM_PATH}/.configured"
            log_info "Setup complete! Next: $0 start"
            ;;

        start)
            if [ ! -f "${VM_PATH}/.configured" ]; then
                log_error "Run '$0 install' first, then '$0 setup'"
                exit 1
            fi
            disk="${VM_PATH}/disk.qcow2"
            start_vm "$disk"
            ;;

        stop)
            stop_vm
            ;;

        status)
            show_status
            ;;

        ssh)
            ssh_key="${VM_PATH}/id_rsa"
            shift
            ssh -i "$ssh_key" -p "$SSH_PORT" \
                -o StrictHostKeyChecking=no \
                -o UserKnownHostsFile=/dev/null \
                root@localhost "$@"
            ;;

        console)
            log_info "Connecting to VM console (Ctrl-A X to exit)..."
            socat -,raw,echo=0,escape=0x01 unix-connect:"${VM_PATH}/qemu.monitor"
            ;;

        logs)
            tail -f "${VM_PATH}/serial.log"
            ;;

        clean)
            stop_vm
            log_warn "Delete VM and all data?"
            read -p "Type 'yes' to confirm: " confirm
            if [ "$confirm" = "yes" ]; then
                rm -rf "$VM_PATH"
                log_info "VM deleted"
            fi
            ;;

        *)
            cat <<EOF
Usage: $0 COMMAND

COMMANDS:
  install  - Create VM and run Alpine installer
  setup    - Configure SSH and qarax deps (after install)
  start    - Start the configured VM
  stop     - Stop the VM
  status   - Show VM status
  ssh      - SSH into the VM
  console  - Connect to QEMU monitor
  logs     - Show serial console logs
  clean    - Delete VM

QUICK START:
  1. $0 install   # Follow prompts to install Alpine
  2. $0 setup     # Follow prompts to configure SSH
  3. $0 start     # Start the VM
  4. $0 ssh       # Connect via SSH

DEPLOY qarax-node:
  cargo build -p qarax-node
  scp -P $SSH_PORT target/debug/qarax-node root@localhost:/usr/local/bin/
  $0 ssh "/usr/local/bin/qarax-node --version"

EOF
            ;;
    esac
}

main "$@"
