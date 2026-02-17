#!/usr/bin/env bash
# Stop and remove the local Docker stack (postgres, qarax, qarax-node) and volumes.
# Use after ./hack/run_local.sh when you're done testing.

set -e

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${REPO_ROOT}/e2e"

echo "Stopping and removing local stack (postgres, qarax, qarax-node) and volumes..."
docker compose down -v
echo "Done."
