#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
FIRMWARE_DIR="$TOOL_ROOT/firmware"

mode="canonical"
recover="if-rom"
force_min_charge="false"
probe_mode="strict"
rom_image="r2"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--mode canonical|dual-diag] [--recover never|if-rom|force]
                         [--force-min-charge true|false] [--probe-mode strict|mac-only] [--rom-image r2|r3|r5]
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
    --recover)
      require_value "$1" "$#"
      recover="${2:-}"
      shift 2
      ;;
    --force-min-charge)
      require_value "$1" "$#"
      force_min_charge="${2:-}"
      shift 2
      ;;
    --probe-mode)
      require_value "$1" "$#"
      probe_mode="${2:-}"
      shift 2
      ;;
    --rom-image)
      require_value "$1" "$#"
      rom_image="${2:-}"
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

case "$force_min_charge" in
  true) features+=("force-min-charge") ;;
  false) ;;
  *)
    echo "Invalid --force-min-charge: $force_min_charge" >&2
    exit 5
    ;;
esac

case "$probe_mode" in
  strict) ;;
  mac-only) features+=("bms-mac-probe-only") ;;
  *)
    echo "Invalid --probe-mode: $probe_mode" >&2
    exit 7
    ;;
esac

case "$rom_image" in
  r2) ;;
  r3) features+=("bms-rom-image-r3") ;;
  r5) features+=("bms-rom-image-r5") ;;
  *)
    echo "Invalid --rom-image: $rom_image" >&2
    exit 8
    ;;
esac

if [[ "$recover" == "force" && "$mode" != "dual-diag" ]]; then
  echo "--recover force requires --mode dual-diag" >&2
  exit 6
fi

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
