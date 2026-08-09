#!/bin/bash
set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# This crate predates the explicit-import rule and still has a broad qualified
# path surface. Keep the exception local until that independent cleanup lands.
STYLE_ENFORCE_EXPLICIT_IMPORTS="${STYLE_ENFORCE_EXPLICIT_IMPORTS:-0}"
export STYLE_ENFORCE_EXPLICIT_IMPORTS
exec env RS_CI_PROJECT_ROOT="$PROJECT_ROOT" "$PROJECT_ROOT/.rs-ci/ci-check.sh" "$@"
