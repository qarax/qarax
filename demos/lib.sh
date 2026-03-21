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

die() { echo -e "${RED}ERROR: $*${NC}" >&2; exit 1; }

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
