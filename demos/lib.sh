#!/usr/bin/env bash
# Shared utilities sourced by qarax demo scripts.
#
# Requires REPO_ROOT to be set by the caller before sourcing:
#   REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
#   source "${REPO_ROOT}/demos/lib.sh"

MUSL_TARGET="x86_64-unknown-linux-musl"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

die() {
	echo -e "${RED}ERROR: $*${NC}" >&2
	exit 1
}

# Verify the qarax API server is reachable. Exits with a clear message if not.
require_server() {
	local server="${1:-http://localhost:8000}"
	if ! curl -sf --max-time 3 "${server}/" >/dev/null 2>&1; then
		die "qarax server not reachable at ${server}\nRun 'make run-local' to start the stack."
	fi
}

# Ensure the qarax stack is running. If the server is not reachable, start it
# automatically via hack/run-local.sh, then wait for it to become ready.
ensure_stack() {
	local server="${1:-http://localhost:8000}"

	if curl -sf --max-time 3 "${server}/" >/dev/null 2>&1; then
		return 0
	fi

	echo -e "${YELLOW}qarax stack not running — starting it now...${NC}"
	echo -e "${DIM}(Running hack/run-local.sh — this may take a few minutes on first run)${NC}"

	bash "${REPO_ROOT}/hack/run-local.sh"

	# Poll until the API responds (up to 30 s of extra grace after run-local exits)
	local elapsed=0
	while [[ $elapsed -lt 30 ]]; do
		if curl -sf --max-time 3 "${server}/" >/dev/null 2>&1; then
			echo -e "${GREEN}Stack is up.${NC}"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Stack started but server still not reachable at ${server}"
}

# Print the path to the qarax CLI binary, searching PATH then the cargo build tree.
find_qarax_bin() {
	if command -v qarax &>/dev/null; then
		echo "qarax"
	elif [[ -x "${REPO_ROOT}/target/${MUSL_TARGET}/debug/qarax" ]]; then
		echo "${REPO_ROOT}/target/${MUSL_TARGET}/debug/qarax"
	elif [[ -x "${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax" ]]; then
		echo "${REPO_ROOT}/target/${MUSL_TARGET}/release/qarax"
	else
		echo ""
	fi
}
