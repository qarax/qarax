#!/bin/bash
# Quick development deployment script for qarax-node
# This script builds qarax-node and deploys it to a test host

set -e

# Configuration
TEST_HOST="${TEST_HOST:-localhost}"
SSH_PORT="${SSH_PORT:-2222}"
SSH_KEY="${SSH_KEY:-}"
BINARY_PATH="${BINARY_PATH:-target/debug/qarax-node}"
BUILD_MODE="${BUILD_MODE:-debug}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Auto-detect SSH key if not provided
detect_ssh_key() {
    if [ -n "$SSH_KEY" ]; then
        echo "$SSH_KEY"
        return
    fi

    local script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local project_root="$(cd "$script_dir/.." && pwd)"
    local auto_key="${project_root}/.vm-hosts/qarax-test-host/id_rsa"

    if [ -f "$auto_key" ]; then
        echo "$auto_key"
    else
        echo ""
    fi
}

# Build qarax-node
build_binary() {
    log_step "Building qarax-node ($BUILD_MODE mode)..."

    if [ "$BUILD_MODE" = "release" ]; then
        cargo build --release -p qarax-node
        BINARY_PATH="target/release/qarax-node"
    else
        cargo build -p qarax-node
        BINARY_PATH="target/debug/qarax-node"
    fi

    if [ ! -f "$BINARY_PATH" ]; then
        log_error "Build failed: $BINARY_PATH not found"
        exit 1
    fi

    local size=$(du -h "$BINARY_PATH" | cut -f1)
    log_info "Built: $BINARY_PATH ($size)"
}

# Deploy binary to test host
deploy_binary() {
    log_step "Deploying to $TEST_HOST:$SSH_PORT..."

    local ssh_opts="-p $SSH_PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    local ssh_key=$(detect_ssh_key)

    if [ -n "$ssh_key" ]; then
        ssh_opts="$ssh_opts -i $ssh_key"
        log_info "Using SSH key: $ssh_key"
    fi

    # Copy binary
    scp $ssh_opts "$BINARY_PATH" "root@$TEST_HOST:/usr/local/bin/qarax-node"

    if [ $? -ne 0 ]; then
        log_error "Failed to copy binary to host"
        exit 1
    fi

    log_info "Binary deployed successfully"
}

# Restart qarax-node service
restart_service() {
    log_step "Restarting qarax-node service..."

    local ssh_opts="-p $SSH_PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    local ssh_key=$(detect_ssh_key)

    if [ -n "$ssh_key" ]; then
        ssh_opts="$ssh_opts -i $ssh_key"
    fi

    ssh $ssh_opts "root@$TEST_HOST" "systemctl restart qarax-node"

    if [ $? -ne 0 ]; then
        log_error "Failed to restart service"
        exit 1
    fi

    log_info "Service restarted"
}

# Check service status
check_status() {
    log_step "Checking qarax-node status..."

    local ssh_opts="-p $SSH_PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    local ssh_key=$(detect_ssh_key)

    if [ -n "$ssh_key" ]; then
        ssh_opts="$ssh_opts -i $ssh_key"
    fi

    ssh $ssh_opts "root@$TEST_HOST" "systemctl status qarax-node --no-pager" || true
}

# Watch logs
watch_logs() {
    log_step "Watching qarax-node logs (Ctrl+C to exit)..."

    local ssh_opts="-p $SSH_PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    local ssh_key=$(detect_ssh_key)

    if [ -n "$ssh_key" ]; then
        ssh_opts="$ssh_opts -i $ssh_key"
    fi

    ssh $ssh_opts "root@$TEST_HOST" "journalctl -u qarax-node -f"
}

# Show usage
usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Quick deployment script for qarax-node development.

OPTIONS:
    -h, --help          Show this help message
    -r, --release       Build in release mode (default: debug)
    -H, --host HOST     Target host (default: localhost)
    -p, --port PORT     SSH port (default: 2222)
    -k, --key PATH      SSH key path (auto-detected if not provided)
    -l, --logs          Watch logs after deployment
    -s, --status        Check service status after deployment

ENVIRONMENT VARIABLES:
    TEST_HOST           Target host (default: localhost)
    SSH_PORT            SSH port (default: 2222)
    SSH_KEY             SSH key path
    BUILD_MODE          Build mode: debug or release (default: debug)

EXAMPLES:
    # Basic deployment
    $0

    # Deploy and watch logs
    $0 --logs

    # Deploy release build
    $0 --release

    # Deploy to custom host
    $0 --host 192.168.1.100 --port 22 --key ~/.ssh/id_rsa

    # Watch mode (requires cargo-watch)
    cargo watch -x 'build -p qarax-node' -s '$0'

EOF
}

# Main script
main() {
    local watch=false
    local show_status=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                usage
                exit 0
                ;;
            -r|--release)
                BUILD_MODE="release"
                shift
                ;;
            -H|--host)
                TEST_HOST="$2"
                shift 2
                ;;
            -p|--port)
                SSH_PORT="$2"
                shift 2
                ;;
            -k|--key)
                SSH_KEY="$2"
                shift 2
                ;;
            -l|--logs)
                watch=true
                shift
                ;;
            -s|--status)
                show_status=true
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done

    echo ""
    log_info "qarax-node Development Deployment"
    log_info "=================================="
    echo ""

    # Execute deployment steps
    build_binary
    deploy_binary
    restart_service

    if [ "$show_status" = true ]; then
        echo ""
        check_status
    fi

    echo ""
    log_info "✅ Deployment complete!"
    log_info ""
    log_info "Test the deployment:"
    log_info "  ssh -p $SSH_PORT root@$TEST_HOST 'qarax-node --version'"
    log_info ""
    log_info "Watch logs:"
    log_info "  $0 --logs"
    echo ""

    if [ "$watch" = true ]; then
        echo ""
        watch_logs
    fi
}

main "$@"
