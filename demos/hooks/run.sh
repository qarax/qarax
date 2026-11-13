#!/usr/bin/env bash
#
# Demo: qarax lifecycle hooks
#
# Starts a small webhook receiver, registers a hook for VMs tagged
# `demo-hooks`, runs a tagged VM through its lifecycle and checks that every
# status transition is delivered with a valid HMAC signature.
#
# Prerequisites:
#   - qarax stack running (make run-local; started automatically if not up)
#   - jq and python3 (the webhook receiver)
#   - docker on Linux, to find the address the qarax container reaches
#     this machine on (or set WEBHOOK_HOST)
#
# Usage:
#   ./demos/hooks/run.sh [--server URL] [--webhook-host HOST] [--webhook-port PORT]
#   ./demos/hooks/run.sh --cleanup   # delete what an earlier run left behind
#   KEEP=1 ./demos/hooks/run.sh      # keep the hook (and the VM, on failure)
#   WEBHOOK_HOST=192.168.1.10 ./demos/hooks/run.sh  # if the auto-detected address doesn't work
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="demo-hooks-"
HOOK_NAME="${PREFIX}webhook"
VM_NAME="${PREFIX}vm"
TAG="demo-hooks"
HOOK_SECRET="demo-hooks-secret"
WEBHOOK_HOST="${WEBHOOK_HOST:-}"
WEBHOOK_PORT="${WEBHOOK_PORT:-9999}"
CLEANUP_ONLY=0

while [[ $# -gt 0 ]]; do
	case $1 in
	--server)
		SERVER="$2"
		shift 2
		;;
	--webhook-host)
		WEBHOOK_HOST="$2"
		shift 2
		;;
	--webhook-port)
		WEBHOOK_PORT="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	--help | -h)
		echo "Usage: $0 [OPTIONS]"
		echo "  --server URL          qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)"
		echo "  --webhook-host HOST   How qarax reaches this machine (default: \$WEBHOOK_HOST, else the"
		echo "                        e2e Docker network gateway on Linux, host.docker.internal elsewhere)"
		echo "  --webhook-port PORT   Local port for the webhook receiver (default: \$WEBHOOK_PORT or 9999)"
		echo "  --cleanup             Delete the demo's hook and VM, then exit"
		echo ""
		echo "KEEP=1 leaves the hook in place at exit."
		exit 0
		;;
	*)
		die "Unknown option: $1"
		;;
	esac
done

# How qarax (in a container) reaches the receiver on this machine: the
# gateway of the stack's Docker network on Linux, host.docker.internal on
# Docker Desktop.
default_webhook_host() {
	if [[ "$(uname -s)" != "Linux" ]]; then
		echo "host.docker.internal"
		return
	fi
	# A lookup fails when docker or that network is missing; try the next.
	docker network inspect e2e_default --format '{{(index .IPAM.Config 0).Gateway}}' 2>/dev/null ||
		docker network inspect bridge --format '{{(index .IPAM.Config 0).Gateway}}' 2>/dev/null ||
		echo "172.17.0.1"
}

# Delete the hook before the VM, so deleting the VM queues no delivery.
remove_leftovers() {
	sweep hook "$PREFIX"
	sweep vm "$PREFIX"
}

receiver_listening() {
	kill -0 "$RECEIVER_PID" 2>/dev/null ||
		die "The webhook receiver exited (is port ${WEBHOOK_PORT} already in use?)"
	curl -s -o /dev/null "http://127.0.0.1:${WEBHOOK_PORT}/"
}

# wait_for_webhook TRANSITION [TIMEOUT]: wait until the receiver has logged a
# delivery of TRANSITION (e.g. created→pending) for the demo VM, and check
# that its signature verified. The hook executor sends every 2 s.
wait_for_webhook() {
	local transition="$1" timeout="${2:-15}"
	if ! wait_until "$timeout" grep -qF "${VM_NAME} ${transition} " "$DELIVERIES"; then
		qarax hook executions "$HOOK_NAME" >&2
		die "No webhook for ${transition} within ${timeout}s. Can qarax reach http://${WEBHOOK_HOST}:${WEBHOOK_PORT}? (set WEBHOOK_HOST)"
	fi
	grep -qF "${VM_NAME} ${transition} verified" "$DELIVERIES" ||
		die "The webhook for ${transition} failed HMAC verification"
}

# The receiver answers before the executor records a delivery.
all_delivered() {
	qarax hook executions "$HOOK_NAME" -o json | jq -e 'all(.[]; .status == "delivered")' >/dev/null
}

command -v jq >/dev/null 2>&1 || die "jq is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required for the webhook receiver"

ensure_stack
require_cli

if ((CLEANUP_ONLY)); then
	remove_leftovers
	ok "Removed the hooks demo's hook and VM"
	exit 0
fi

[[ -n "$WEBHOOK_HOST" ]] || WEBHOOK_HOST=$(default_webhook_host)

banner "Lifecycle Hooks Demo"

step "Removing leftovers from earlier runs..."
remove_leftovers

DELIVERIES=$(mktemp)
RECEIVER_PID=""

cleanup() {
	echo
	if [[ -n "$RECEIVER_PID" ]]; then
		# Already gone if it failed to start.
		kill "$RECEIVER_PID" 2>/dev/null || true
	fi
	rm -f "$DELIVERIES"
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving hook '${HOOK_NAME}' (and the VM, if any) in place; remove with: $0 --cleanup"
		return
	fi
	step "Cleaning up..."
	remove_leftovers
	info "Done."
}
trap cleanup EXIT

step "Starting webhook receiver on port ${WEBHOOK_PORT}..."
HOOK_SECRET="$HOOK_SECRET" WEBHOOK_PORT="$WEBHOOK_PORT" DELIVERIES="$DELIVERIES" \
	python3 -u "${REPO_ROOT}/demos/hooks/receiver.py" &
RECEIVER_PID=$!
wait_until 5 receiver_listening || die "The webhook receiver did not start listening on port ${WEBHOOK_PORT}"
info "qarax will POST to http://${WEBHOOK_HOST}:${WEBHOOK_PORT}/webhook"

step "Registering a hook for VMs tagged '${TAG}', signed with a shared secret..."
run qarax hook create \
	--name "$HOOK_NAME" \
	--url "http://${WEBHOOK_HOST}:${WEBHOOK_PORT}/webhook" \
	--scope tag \
	--scope-value "$TAG" \
	--secret "$HOOK_SECRET"
run qarax hook get "$HOOK_NAME"

banner "Running VM Lifecycle  (watch for ⚡ webhooks below)"

step "Creating VM '${VM_NAME}' (creation is not a status change, so no webhook)..."
run qarax vm create --name "$VM_NAME" --tag "$TAG" --vcpus 1 --memory 256MiB

step "Starting VM..."
run qarax vm start "$VM_NAME"
wait_for_webhook "created→pending"
wait_for_webhook "pending→running"

step "Pausing VM..."
run qarax vm pause "$VM_NAME"
wait_for_webhook "running→paused"

step "Resuming VM..."
run qarax vm resume "$VM_NAME"
wait_for_webhook "paused→running"

step "Stopping VM..."
run qarax vm stop "$VM_NAME"
wait_for_webhook "running→shutdown"

step "Deleting VM..."
run qarax vm delete "$VM_NAME"
wait_for_webhook "shutdown→deleted"

banner "Hook Execution History"
wait_until 5 all_delivered || warn "Some executions are not marked delivered yet"
run qarax hook executions "$HOOK_NAME"

banner "Demo Complete"
ok "All 6 transitions were delivered, each with a valid sha256 HMAC signature"
