#!/usr/bin/env bash
# Stop and remove the local Docker stack (postgres, qarax, qarax-node) and volumes.
# Use after ./hack/run-local.sh when you're done testing.

set -e

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "Stopping and removing local stack (postgres, qarax, qarax-node) and volumes..."
bash "${REPO_ROOT}/hack/run-local.sh" --cleanup
echo "Done."
