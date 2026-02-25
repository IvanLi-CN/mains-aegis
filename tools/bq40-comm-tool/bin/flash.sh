#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

if [[ $# -gt 0 ]]; then
  case "$1" in
    -h|--help)
      echo "Usage: $(basename "$0")"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
fi

(
  cd "$TOOL_ROOT"
  mcu-agentd --non-interactive flash esp
)
