#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

if [[ -n "${CODE_REVIEW_GRAPH_PYTHON:-}" ]]; then
  exec "${CODE_REVIEW_GRAPH_PYTHON}" -m code_review_graph serve \
    --repo "${REPO_ROOT}" --auto-watch "$@"
fi

if command -v code-review-graph >/dev/null 2>&1; then
  exec code-review-graph serve --repo "${REPO_ROOT}" --auto-watch "$@"
fi

UV_TOOL_PYTHON="${HOME}/.local/share/uv/tools/code-review-graph/bin/python"
if [[ -x "${UV_TOOL_PYTHON}" ]]; then
  exec "${UV_TOOL_PYTHON}" -m code_review_graph serve \
    --repo "${REPO_ROOT}" --auto-watch "$@"
fi

if command -v uvx >/dev/null 2>&1; then
  exec uvx --from code-review-graph code-review-graph serve \
    --repo "${REPO_ROOT}" --auto-watch "$@"
fi

echo "code-review-graph is unavailable: install it or set CODE_REVIEW_GRAPH_PYTHON" >&2
exit 127
