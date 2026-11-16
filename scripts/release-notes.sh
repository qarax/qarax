#!/bin/sh
# Prints VERSION's section of CHANGELOG.md (without its heading), for the
# release notes. Fails if there is none.
#
# Usage: release-notes.sh VERSION
set -eu

version=$1
changelog="$(dirname "$0")/../CHANGELOG.md"

awk -v version="$version" '
	/^## \[/ {
		if (found) exit
		if (index($0, "## [" version "]") == 1) { found = 1; next }
	}
	found { print }
	END { if (!found) exit 1 }
' "$changelog" || {
	echo "CHANGELOG.md has no section for $version" >&2
	exit 1
}
