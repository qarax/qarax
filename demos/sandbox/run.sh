#!/usr/bin/env bash
#
# Demo: qarax sandboxes
#
# Sandboxes are ephemeral VMs created from a VM template and reaped once they
# have been idle for longer than their idle timeout. The demo:
#
#   1. Gets or creates a Firecracker and a Cloud Hypervisor template
#   2. Creates a sandbox from each and times how long each takes to be ready
#   3. Configures a prewarmed sandbox pool for the Firecracker template
#   4. Inspects the Firecracker sandbox and runs a command inside it
#   5. Claims a second sandbox from the pool
#   6. Deletes the first sandbox by hand
#   7. Waits for the second sandbox to be reaped after its idle timeout
#
# With --template, the demo uses the caller's template instead and skips the
# Cloud Hypervisor comparison.
#
# The templates have no boot source: their VMs boot the control plane's
# default kernel and initramfs (VM_KERNEL / VM_INITRAMFS on the local stack),
# whose qarax-init runs the guest agent that `sandbox exec` talks to.
#
# Usage:
#   ./demos/sandbox/run.sh [--server URL] [--template NAME] [--idle-timeout SECS]
#   ./demos/sandbox/run.sh --cleanup   # remove the demo's sandboxes, pools and templates
#   KEEP=1 ./demos/sandbox/run.sh      # leave the sandboxes and the pool in place at exit
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=demos/lib.sh
source "${REPO_ROOT}/demos/lib.sh"

PREFIX="sandbox-demo-"
FC_TEMPLATE="${PREFIX}fc"
CH_TEMPLATE="${PREFIX}ch"
SANDBOX1="${PREFIX}1"
CH_SANDBOX="${PREFIX}ch-1"
SANDBOX2="${PREFIX}2"
IDLE_TIMEOUT="${SANDBOX_IDLE_TIMEOUT:-30}"
POOL_MIN_READY="${SANDBOX_POOL_MIN_READY:-1}"
READY_TIMEOUT="${SANDBOX_READY_TIMEOUT:-300}"
GUEST_AGENT_TIMEOUT=30
TEMPLATE="$FC_TEMPLATE"
MANAGED_TEMPLATES=1
CLEANUP_ONLY=0

usage() {
	cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --server URL         qarax API URL (default: \$QARAX_SERVER or http://localhost:8000)
  --template NAME      Use an existing VM template instead of the demo's Firecracker and
                       Cloud Hypervisor templates (skips the comparison)
  --idle-timeout SECS  Idle timeout of the sandbox claimed from the pool, which the demo
                       waits to see reaped (default: \$SANDBOX_IDLE_TIMEOUT or 30)
  --cleanup            Remove the demo's sandboxes, sandbox pools and templates, then exit

Environment: QARAX_TOKEN, KEEP=1 (leave sandboxes and the pool at exit),
             SANDBOX_POOL_MIN_READY (default 1), SANDBOX_READY_TIMEOUT (default 300)
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--server)
		SERVER="$2"
		shift 2
		;;
	--template)
		TEMPLATE="$2"
		MANAGED_TEMPLATES=0
		shift 2
		;;
	--idle-timeout)
		IDLE_TIMEOUT="$2"
		shift 2
		;;
	--cleanup)
		CLEANUP_ONLY=1
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		die "unknown option: $1 (see --help)"
		;;
	esac
done

# Delete the sandbox pools of the demo's templates. A pool keeps standby VMs
# running, a template with a pool can't be deleted, and a leftover pool would
# turn the cold-start measurement into a claim.
sweep_pools() {
	local id
	for id in $(qarax sandbox pool list -o json |
		jq -r --arg p "$PREFIX" '.[] | select(.vm_template_name | startswith($p)) | .vm_template_id'); do
		qarax sandbox pool delete --template "$id" >/dev/null
	done
}

# Get or create a 1 vCPU / 256 MiB template for HYPERVISOR.
ensure_template() {
	local name="$1" hypervisor="$2"
	if exists vm-template "$name"; then
		ok "Template ${name} exists"
	else
		run qarax vm-template create --name "$name" --hypervisor "$hypervisor" \
			--vcpus 1 --memory 268435456
	fi
}

# Milliseconds since the epoch. EPOCHREALTIME's decimal separator follows the
# locale, so drop whatever it is.
now_ms() {
	local us="${EPOCHREALTIME//[!0-9]/}"
	echo $((us / 1000))
}

# sandbox_ready ID: true once the sandbox is ready; dies if it failed to start.
sandbox_ready() {
	local json
	json=$(qarax sandbox get "$1" -o json) || die "could not read sandbox $1"
	case "$(jq -r .status <<<"$json")" in
	ready) return 0 ;;
	error) die "sandbox $1 failed to start: $(jq -r .error_message <<<"$json")" ;;
	*) return 1 ;;
	esac
}

# create_timed TEMPLATE NAME [ARGS...]: create a sandbox and poll it every
# 0.1 s until it is ready. Sets SANDBOX_ID, SANDBOX_JOB and READY_MS.
# (`sandbox create --wait` backs off to 2 s between polls: fine for waiting,
# too coarse for comparing boot times.)
create_timed() {
	local template="$1" name="$2" start deadline json
	shift 2
	info "\$ qarax sandbox create --template ${template} --name ${name}${*:+ $*}"
	start=$(now_ms)
	json=$(qarax sandbox create --template "$template" --name "$name" "$@" -o json)
	SANDBOX_ID=$(jq -r .id <<<"$json")
	SANDBOX_JOB=$(jq -r .job_id <<<"$json")
	deadline=$((SECONDS + READY_TIMEOUT))
	until sandbox_ready "$SANDBOX_ID"; do
		((SECONDS < deadline)) || die "sandbox ${name} not ready after ${READY_TIMEOUT}s"
		sleep 0.1
	done
	READY_MS=$(($(now_ms) - start))
}

# READY means the sandbox VM is running, not that the guest has booted far
# enough for its agent to listen: `sandbox exec` fails until then, so errors
# are expected here.
sandbox_agent_ready() {
	qarax sandbox exec "$1" -- true >/dev/null 2>&1
}

pool_filled() {
	local ready
	ready=$(qarax sandbox pool get --template "$TEMPLATE_ID" -o json | jq .current_ready) ||
		die "could not read the sandbox pool of ${TEMPLATE}"
	((ready >= POOL_MIN_READY))
}

# sandbox_gone ID: true once GET /sandboxes/ID is a 404. (The CLI can't tell a
# 404 from other errors, hence curl.) Any other status fails the demo, so an
# API error isn't mistaken for a reap.
sandbox_gone() {
	local code
	code=$(api_curl -s -o /dev/null -w '%{http_code}' "${SERVER}/sandboxes/$1")
	case "$code" in
	404) return 0 ;;
	200) return 1 ;;
	*) die "GET /sandboxes/$1 returned HTTP ${code}" ;;
	esac
}

banner "Sandbox Demo"

if ((CLEANUP_ONLY)); then
	require_server
	require_cli
	step "Removing the demo's sandboxes, sandbox pools and templates"
	sweep sandbox "$PREFIX"
	sweep vm "$PREFIX"
	sweep_pools
	sweep vm-template "$PREFIX"
	ok "Done"
	exit 0
fi

ensure_stack
require_cli

step "Clearing what an earlier run left behind"
sweep sandbox "$PREFIX"
sweep vm "$PREFIX"
sweep_pools

POOL_SET=0
cleanup() {
	if [[ "$KEEP" == 1 ]]; then
		info "KEEP=1: leaving the ${PREFIX}* sandboxes and the sandbox pool in place ($0 --cleanup removes them)."
		return
	fi
	step "Cleaning up"
	sweep sandbox "$PREFIX"
	if ((POOL_SET)); then
		if [[ -n "$ORIG_MIN_READY" ]]; then
			qarax sandbox pool set --template "$TEMPLATE_ID" --min-ready "$ORIG_MIN_READY" >/dev/null
		else
			qarax sandbox pool delete --template "$TEMPLATE_ID" >/dev/null
		fi
	fi
	ok "Done"
}
trap cleanup EXIT

if ((MANAGED_TEMPLATES)); then
	step "Templates (kept between runs)"
	ensure_template "$FC_TEMPLATE" firecracker
	ensure_template "$CH_TEMPLATE" cloud_hv
fi
TEMPLATE_ID=$(qarax vm-template get "$TEMPLATE" -o json | jq -r .id)
# A caller's template may already have a pool; its min_ready is restored at exit.
ORIG_MIN_READY=$(qarax sandbox pool list -o json |
	jq -r --arg id "$TEMPLATE_ID" '.[] | select(.vm_template_id == $id) | .min_ready')

step "Cold start: create ${SANDBOX1} from ${TEMPLATE}"
create_timed "$TEMPLATE" "$SANDBOX1"
COLD_MS="$READY_MS"
ok "${SANDBOX1} ready in ${COLD_MS} ms"

if ((MANAGED_TEMPLATES)); then
	step "The same guest on Cloud Hypervisor, for comparison"
	create_timed "$CH_TEMPLATE" "$CH_SANDBOX"
	CH_MS="$READY_MS"
	ok "${CH_SANDBOX} ready in ${CH_MS} ms"
	run qarax sandbox delete "$CH_SANDBOX"
fi

step "Prewarm a pool of ${POOL_MIN_READY} standby sandbox(es) for ${TEMPLATE}"
if [[ -n "$ORIG_MIN_READY" ]]; then
	info "${TEMPLATE} already has a pool (min_ready=${ORIG_MIN_READY}); it is restored at exit."
fi
POOL_SET=1
run qarax sandbox pool set --template "$TEMPLATE" --min-ready "$POOL_MIN_READY"
info "The pool manager boots the standby in the background while the demo goes on."

step "Inspect ${SANDBOX1}"
run qarax sandbox get "$SANDBOX1"

step "Run a command inside ${SANDBOX1}"
info "exec and keepalive reset a sandbox's idle clock; get does not."
run qarax sandbox keepalive "$SANDBOX1"
wait_until "$GUEST_AGENT_TIMEOUT" sandbox_agent_ready "$SANDBOX1" ||
	die "the guest agent in ${SANDBOX1} did not answer within ${GUEST_AGENT_TIMEOUT}s"
run qarax sandbox exec "$SANDBOX1" -- uname -a

step "Wait for the pool's standby sandbox"
if ! wait_until "$READY_TIMEOUT" pool_filled; then
	qarax sandbox pool get --template "$TEMPLATE"
	die "the sandbox pool of ${TEMPLATE} did not fill within ${READY_TIMEOUT}s"
fi
run qarax sandbox pool get --template "$TEMPLATE"

step "Claim ${SANDBOX2} from the pool (idle timeout ${IDLE_TIMEOUT}s)"
create_timed "$TEMPLATE" "$SANDBOX2" --idle-timeout "$IDLE_TIMEOUT"
SANDBOX2_ID="$SANDBOX_ID"
WARM_MS="$READY_MS"
CLAIMED_AT=$SECONDS
[[ "$(qarax job get "$SANDBOX_JOB" -o json | jq -r .job_type)" == sandbox_claim ]] ||
	die "${SANDBOX2} was booted, not claimed from the pool"
ok "${SANDBOX2} claimed from the pool, ready in ${WARM_MS} ms"
info "The pool refills in the background:"
run qarax sandbox pool get --template "$TEMPLATE"

step "Delete ${SANDBOX1} by hand"
run qarax sandbox delete "$SANDBOX1"
run qarax sandbox list

step "Wait for the reaper to remove ${SANDBOX2}"
info "The reaper runs every 10s (sandbox.reaper_interval_secs) and deletes sandboxes idle for longer than their timeout."
wait_until $((IDLE_TIMEOUT + 30)) sandbox_gone "$SANDBOX2_ID" ||
	die "${SANDBOX2} still exists $((SECONDS - CLAIMED_AT))s after it was claimed"
ok "${SANDBOX2} reaped $((SECONDS - CLAIMED_AT))s after it was claimed"

banner "Demo Complete"
echo "Time from 'sandbox create' to ready in this run:"
printf '  %-30s %6s ms\n' "${TEMPLATE}, booted" "$COLD_MS"
if ((MANAGED_TEMPLATES)); then
	printf '  %-30s %6s ms\n' "${CH_TEMPLATE}, booted" "$CH_MS"
fi
printf '  %-30s %6s ms\n' "${TEMPLATE}, claimed from pool" "$WARM_MS"
echo ""
echo "Useful commands:"
echo "  qarax sandbox create --template T --wait   # create a sandbox and wait until it is ready"
echo "  qarax sandbox exec <name> -- cmd           # run a command inside a sandbox"
echo "  qarax sandbox keepalive <name>             # reset the idle clock"
echo "  qarax sandbox pool get --template T        # inspect a prewarmed pool"
echo "  $0 --cleanup        # remove the demo's templates too"
