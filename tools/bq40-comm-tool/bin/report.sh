#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

mode="canonical"
duration_sec=120
monitor_file=""
report_out=""

usage() {
  cat <<USAGE
Usage: $(basename "$0") --monitor-file PATH [--mode canonical|dual-diag] [--duration-sec N] [--report-out DIR]
USAGE
}

require_value() {
  local opt="$1"
  local argc="$2"
  if (( argc < 2 )); then
    echo "Option $opt requires a value" >&2
    usage >&2
    exit 2
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      require_value "$1" "$#"
      mode="${2:-}"
      shift 2
      ;;
    --duration-sec)
      require_value "$1" "$#"
      duration_sec="${2:-}"
      shift 2
      ;;
    --monitor-file)
      require_value "$1" "$#"
      monitor_file="${2:-}"
      shift 2
      ;;
    --report-out)
      require_value "$1" "$#"
      report_out="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$monitor_file" ]]; then
  echo "--monitor-file is required" >&2
  exit 3
fi

if [[ -z "$report_out" ]]; then
  ts=$(date +"%Y%m%d_%H%M%S")
  report_out="$TOOL_ROOT/reports/$ts"
fi

python3 "$SCRIPT_DIR/report_parser.py" \
  --mode "$mode" \
  --duration-sec "$duration_sec" \
  --monitor-file "$monitor_file" \
  --report-out "$report_out"
