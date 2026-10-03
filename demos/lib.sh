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

# API token for the control plane. hack/run-local.sh reuses
# e2e/docker-compose.yml, which enables token auth with
# AUTH_TOKENS=${QARAX_TEST_TOKEN:-e2e-test-token}; mirror that default so
# demos work against the local stack out of the box. Exported so the qarax
# CLI (which reads QARAX_TOKEN) authenticates too. Harmless when auth is off.
: "${QARAX_TOKEN:=${QARAX_TEST_TOKEN:-e2e-test-token}}"
export QARAX_TOKEN

# curl wrapper for qarax API calls: adds the bearer token header.
api_curl() {
	curl -H "Authorization: Bearer ${QARAX_TOKEN}" "$@"
}

die() {
	echo -e "${RED}ERROR: $*${NC}" >&2
	exit 1
}

# Verify the token is accepted. "/" is public, so probe an authenticated route.
require_auth() {
	local server="${1:-http://localhost:8000}"
	local code
	code=$(api_curl -s -o /dev/null -w '%{http_code}' --max-time 5 "${server}/hosts" 2>/dev/null || true)
	if [[ "$code" == "401" || "$code" == "403" ]]; then
		die "qarax server at ${server} rejected the API token (HTTP ${code}).\nSet QARAX_TOKEN to one of the server's AUTH_TOKENS."
	fi
}

# Verify the qarax API server is reachable and the token works.
# Exits with a clear message if not.
require_server() {
	local server="${1:-http://localhost:8000}"
	if ! curl -sf --max-time 3 "${server}/" >/dev/null 2>&1; then
		die "qarax server not reachable at ${server}\nRun 'make run-local' to start the stack."
	fi
	require_auth "$server"
}

# Ensure the qarax stack is running. If the server is not reachable, start it
# automatically via hack/run-local.sh, then wait for it to become ready.
ensure_stack() {
	local server="${1:-http://localhost:8000}"

	if curl -sf --max-time 3 "${server}/" >/dev/null 2>&1; then
		require_auth "$server"
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
			require_auth "$server"
			return 0
		fi
		sleep 2
		elapsed=$((elapsed + 2))
	done

	die "Stack started but server still not reachable at ${server}"
}

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
	else
		echo ""
	fi
}
