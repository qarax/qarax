#!/usr/bin/env bash
# Shared utilities sourced by qarax demo scripts.
#
# Requires REPO_ROOT to be set by the caller before sourcing:
#   REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
#   source "${REPO_ROOT}/demos/lib.sh"
#
# Conventions every demo follows:
#   - SERVER is the API URL ($QARAX_SERVER, default http://localhost:8000).
#   - Resources are named with a fixed per-demo prefix, so a re-run (or
#     `--cleanup`) can find and remove what an interrupted run left behind.
#   - KEEP=1 leaves the demo's resources in place at exit for inspection.

MUSL_TARGET="x86_64-unknown-linux-musl"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

SERVER="${QARAX_SERVER:-http://localhost:8000}"
KEEP="${KEEP:-0}"

# API token for the control plane. The local stack (e2e/docker-compose.yml)
# enables token auth with AUTH_TOKENS=${QARAX_TEST_TOKEN:-e2e-test-token};
# mirror that default. Exported so the qarax CLI (which reads QARAX_TOKEN)
# authenticates too. Harmless when auth is off.
: "${QARAX_TOKEN:=${QARAX_TEST_TOKEN:-e2e-test-token}}"
export QARAX_TOKEN

# e2e/docker-compose.yml interpolates these into node image build args.
: "${CLOUD_HYPERVISOR_VERSION:=$(tr -d '\n' <"${REPO_ROOT}/versions/cloud-hypervisor-version")}"
: "${FIRECRACKER_VERSION:=$(tr -d '\n' <"${REPO_ROOT}/versions/firecracker-version")}"
export CLOUD_HYPERVISOR_VERSION FIRECRACKER_VERSION

# ── Output ───────────────────────────────────────────────────────────────────

banner() {
	echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}"
	echo -e "${BOLD}${CYAN}  $1${NC}"
	echo -e "${BOLD}${CYAN}══════════════════════════════════════════════════════════════${NC}\n"
}
step() { echo -e "\n${GREEN}▸${NC} ${BOLD}$1${NC}"; }
info() { echo -e "  ${DIM}$1${NC}"; }
ok() { echo -e "  ${GREEN}✓${NC} $1"; }
warn() { echo -e "  ${YELLOW}!${NC} $1" >&2; }

# Echo a command, then run it. Use it for the commands the demo is showing
# off, so what's printed is exactly what ran.
run() {
	echo -e "  ${DIM}\$ $*${NC}"
	"$@"
}

die() {
	echo -e "${RED}ERROR: $*${NC}" >&2
	exit 1
}

# ── Stack ────────────────────────────────────────────────────────────────────

# curl wrapper for qarax API calls: adds the bearer token header.
api_curl() {
	curl -H "Authorization: Bearer ${QARAX_TOKEN}" "$@"
}

# Verify the token is accepted. "/" is public, so probe an authenticated route.
require_auth() {
	local code
	code=$(api_curl -s -o /dev/null -w '%{http_code}' --max-time 5 "${SERVER}/hosts" 2>/dev/null || true)
	if [[ "$code" == "401" || "$code" == "403" ]]; then
		die "qarax server at ${SERVER} rejected the API token (HTTP ${code}).\nSet QARAX_TOKEN to one of the server's AUTH_TOKENS."
	fi
}

server_reachable() {
	curl -sf --max-time 3 "${SERVER}/" >/dev/null 2>&1
}

# Verify the API server is reachable and the token works.
require_server() {
	server_reachable || die "qarax server not reachable at ${SERVER}\nRun 'make run-local' to start the stack."
	require_auth
}

# Start the local stack with hack/run-local.sh if the API isn't reachable.
ensure_stack() {
	if ! server_reachable; then
		echo -e "${YELLOW}qarax stack not running — starting it now...${NC}"
		echo -e "${DIM}(hack/run-local.sh — a few minutes on first run)${NC}"
		bash "${REPO_ROOT}/hack/run-local.sh"
	fi
	require_server
}

# docker compose against the local stack's compose file.
compose() {
	docker compose -f "${REPO_ROOT}/e2e/docker-compose.yml" "$@"
}

# Run a command inside a local-stack container (e.g. qarax-node). Resolves
# the container once instead of re-parsing the compose file per call.
node_exec() {
	local service="$1" cid
	shift
	cid=$(compose ps -q "$service")
	[[ -n "$cid" ]] || die "compose service ${service} is not running"
	docker exec "$cid" "$@"
}

# Map a registered host's address to the compose service running it.
node_service_for_address() {
	case "$1" in
	qarax-node | qarax-node-2) echo "$1" ;;
	*) return 1 ;;
	esac
}

# ── CLI ──────────────────────────────────────────────────────────────────────

# Print the path to the qarax CLI binary: the newest of the cargo debug/release
# builds, falling back to PATH. Picking the newest avoids running a stale debug
# binary after a fresh release build (or vice versa).
find_qarax_bin() {
	local debug="${REPO_ROOT}/target/${MUSL_TARGET}/debug/qarax"
	local release="${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax"
	if [[ -x "$debug" && -x "$release" ]]; then
		if [[ "$release" -nt "$debug" ]]; then echo "$release"; else echo "$debug"; fi
	elif [[ -x "$debug" ]]; then
		echo "$debug"
	elif [[ -x "$release" ]]; then
		echo "$release"
	elif command -v qarax &>/dev/null; then
		echo "qarax"
	fi
}

# Build the CLI from this checkout (a no-op when it's current) and set
# QARAX_BIN. Without cargo, fall back to a qarax on PATH.
require_cli() {
	if command -v cargo >/dev/null 2>&1; then
		(cd "$REPO_ROOT" && cargo build -q -p cli) || die "failed to build the qarax CLI"
	fi
	QARAX_BIN="$(find_qarax_bin)"
	[[ -n "$QARAX_BIN" ]] || die "qarax CLI not found: install cargo, or put qarax on PATH"
}

# The CLI, pointed at SERVER. Commands printed through `run qarax ...` read
# the same as what a user would type.
qarax() {
	"$QARAX_BIN" --server "$SERVER" "$@"
}

# ── Waiting ──────────────────────────────────────────────────────────────────

# wait_until TIMEOUT_SECONDS CMD...: run CMD every 0.5 s until it succeeds.
# Returns 1 on timeout.
wait_until() {
	local deadline=$((SECONDS + $1))
	shift
	until "$@"; do
		((SECONDS < deadline)) || return 1
		sleep 0.5
	done
}

vm_status() {
	qarax vm get "$1" -o json | jq -r '.status'
}

# wait_vm_status VM STATUS [TIMEOUT]: for transitions that happen in the
# background (HA failover, evacuation). Fails fast if the VM crashes.
# Synchronous commands (create, start, stop, pause, resume, delete) don't need it.
wait_vm_status() {
	local vm="$1" want="$2" timeout="${3:-120}" deadline status
	deadline=$((SECONDS + timeout))
	while true; do
		status=$(vm_status "$vm")
		[[ "$status" == "$want" ]] && return 0
		[[ "$status" == "unknown" ]] && die "VM ${vm} entered 'unknown' status while waiting for ${want}"
		((SECONDS < deadline)) || die "VM ${vm} still '${status}' after ${timeout}s (wanted ${want})"
		sleep 0.5
	done
}

# start_vms VM...: start several VMs concurrently and wait for all of them.
# Each `vm start` blocks until its VM runs; a failure shows that VM's output.
start_vms() {
	local vms=("$@") pids=() logs=() i failed=0
	for i in "${!vms[@]}"; do
		logs[i]=$(mktemp)
		qarax vm start "${vms[$i]}" >"${logs[$i]}" 2>&1 &
		pids[i]=$!
	done
	for i in "${!vms[@]}"; do
		if wait "${pids[$i]}"; then
			ok "${vms[$i]} running"
		else
			failed=1
			warn "${vms[$i]} failed to start:"
			sed 's/^/    /' "${logs[$i]}" >&2
		fi
		rm -f "${logs[$i]}"
	done
	((failed == 0)) || die "VM start failed"
}

# ── Hosts ────────────────────────────────────────────────────────────────────

up_host_count() {
	qarax host list -o json | jq '[.[] | select(.status == "up")] | length'
}

# Print the name of the first UP host.
first_up_host() {
	local name
	name=$(qarax host list -o json | jq -r '[.[] | select(.status == "up")][0].name // empty')
	[[ -n "$name" ]] || die "no UP host registered at ${SERVER}"
	echo "$name"
}

# ensure_up_hosts N: make sure N hosts are UP. On the local stack the second
# node container (qarax-node-2, compose profile "multi-node") is started and
# registered on demand.
ensure_up_hosts() {
	local want="$1" have
	have=$(up_host_count)
	((have >= want)) && return 0
	if ((want == 2)) && server_is_local; then
		info "Starting and registering the second local node (qarax-node-2)..."
		compose --profile multi-node up -d --wait --no-recreate qarax-node-2 >/dev/null
		bash "${REPO_ROOT}/e2e/setup_host.sh" "$SERVER" qarax-node-2 50051 local-node-2 >/dev/null
		have=$(up_host_count)
	fi
	((have >= want)) || die "this demo needs ${want} UP hosts; ${SERVER} has ${have}"
}

server_is_local() {
	[[ "$SERVER" == "http://localhost:8000" || "$SERVER" == "http://127.0.0.1:8000" ]]
}

# ── Cleanup ──────────────────────────────────────────────────────────────────

# sweep RESOURCE PREFIX: delete every RESOURCE (a CLI noun such as vm,
# network, security-group, vm-template, boot-source, hook) whose name starts
# with PREFIX. Used at start-up to clear an interrupted run, and on exit.
sweep() {
	local resource="$1" prefix="$2" id
	for id in $(qarax "$resource" list -o json |
		jq -r --arg p "$prefix" '.[] | select(.name | startswith($p)) | .id'); do
		qarax "$resource" delete "$id" >/dev/null
	done
}
