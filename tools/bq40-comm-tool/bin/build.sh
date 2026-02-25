#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
FIRMWARE_DIR="$TOOL_ROOT/firmware"

mode="canonical"
recover="if-rom"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--mode canonical|dual-diag] [--recover never|if-rom|force]
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      mode="${2:-}"
      shift 2
      ;;
    --recover)
      recover="${2:-}"
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

features=()
case "$mode" in
  canonical) ;;
  dual-diag) features+=("bms-dual-probe-diag") ;;
  *)
    echo "Invalid --mode: $mode" >&2
    exit 3
    ;;
esac

case "$recover" in
  never) features+=("bms-rom-recover-disable") ;;
  if-rom) ;;
  force) features+=("bms-rom-recover-force") ;;
  *)
    echo "Invalid --recover: $recover" >&2
    exit 4
    ;;
esac

build_cmd=(cargo build --release)
if [[ ${#features[@]} -gt 0 ]]; then
  feature_csv=$(IFS=, ; echo "${features[*]}")
  build_cmd+=(--features "$feature_csv")
fi

(
  cd "$FIRMWARE_DIR"
  echo "+ ${build_cmd[*]}"
  "${build_cmd[@]}"
)
