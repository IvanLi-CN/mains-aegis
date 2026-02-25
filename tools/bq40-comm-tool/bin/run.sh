#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

subcommand="${1:-}"
if [[ -z "$subcommand" ]]; then
  echo "Usage: $(basename "$0") <diagnose|recover|verify> [options]" >&2
  exit 2
fi
if [[ "$subcommand" == "-h" || "$subcommand" == "--help" ]]; then
  subcommand="help"
fi
shift || true

mode="canonical"
duration_sec=120
flash="true"
recover_policy="if-rom"
monitor_file=""
report_out=""

usage() {
  cat <<USAGE
Usage:
  $(basename "$0") diagnose [--mode canonical|dual-diag] [--duration-sec N] [--flash true|false] [--monitor-file PATH] [--report-out DIR]
  $(basename "$0") recover  [--mode canonical|dual-diag] [--duration-sec N] [--flash true|false] [--recover never|if-rom|force] [--monitor-file PATH] [--report-out DIR]
  $(basename "$0") verify   --monitor-file PATH [--mode canonical|dual-diag] [--duration-sec N] [--report-out DIR]
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      mode="${2:-}"
      shift 2
      ;;
    --duration-sec)
      duration_sec="${2:-}"
      shift 2
      ;;
    --flash)
      flash="${2:-}"
      shift 2
      ;;
    --recover)
      recover_policy="${2:-}"
      shift 2
      ;;
    --monitor-file)
      monitor_file="${2:-}"
      shift 2
      ;;
    --report-out)
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

case "$subcommand" in
  help)
    usage
    exit 0
    ;;
  diagnose)
    recover_policy="never"
    ;;
  recover)
    ;;
  verify)
    ;;
  *)
    echo "Unknown subcommand: $subcommand" >&2
    usage >&2
    exit 3
    ;;
esac

case "$mode" in
  canonical|dual-diag) ;;
  *)
    echo "Invalid --mode: $mode" >&2
    exit 4
    ;;
esac

if ! [[ "$duration_sec" =~ ^[0-9]+$ ]] || [[ "$duration_sec" -lt 1 ]]; then
  echo "Invalid --duration-sec: $duration_sec" >&2
  exit 5
fi

case "$flash" in
  true|false) ;;
  *)
    echo "Invalid --flash: $flash" >&2
    exit 6
    ;;
esac

case "$recover_policy" in
  never|if-rom|force) ;;
  *)
    echo "Invalid --recover: $recover_policy" >&2
    exit 7
    ;;
esac

if [[ "$subcommand" == "verify" ]]; then
  if [[ -z "$monitor_file" ]]; then
    echo "verify mode requires --monitor-file" >&2
    exit 8
  fi
else
  "$SCRIPT_DIR/build.sh" --mode "$mode" --recover "$recover_policy"

  if [[ "$flash" == "true" ]]; then
    "$SCRIPT_DIR/flash.sh"
  fi

  if [[ -n "$monitor_file" ]]; then
    monitor_file=$("$SCRIPT_DIR/monitor.sh" --duration-sec "$duration_sec" --output "$monitor_file")
  else
    monitor_file=$("$SCRIPT_DIR/monitor.sh" --duration-sec "$duration_sec")
  fi
fi

report_args=(
  --mode "$mode"
  --duration-sec "$duration_sec"
  --monitor-file "$monitor_file"
)
if [[ -n "$report_out" ]]; then
  report_args+=(--report-out "$report_out")
fi

"$SCRIPT_DIR/report.sh" "${report_args[@]}"
