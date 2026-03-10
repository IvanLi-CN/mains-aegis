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
force_min_charge="false"
probe_mode="strict"
rom_image="r2"
monitor_file=""
report_out=""
flash_arg_set="false"
recover_arg_set="false"
force_min_charge_arg_set="false"
probe_mode_arg_set="false"
rom_image_arg_set="false"
duration_arg_set="false"
mode_arg_set="false"
MAIN_LOOP_QUANTUM_SEC=2
WORKING_INFO_PERIOD_SEC=5
MIN_VALID_STREAK=10
REPOWER_OFF_WINDOW_SEC=10
MIN_CHARGE_SETTLE_SEC=2
POST_FLASH_BOOT_QUIET_SEC=10
POST_FLASH_RESUME_WINDOW_SEC=30
ROM_FLASH_IMAGE_BYTES=$((8192 + 57344 + 34 + 34))
ROM_FLASH_BLOCK_BYTES=64
ROM_FLASH_BLOCK_ONWIRE_BYTES=67
ROM_FLASH_BITS_PER_BYTE=9
I2C_SLOW_BUS_BPS=$((25 * 1000))
ROM_FLASH_WRITE_GAP_MS=10
ROM_FLASH_FIXED_LATENCY_SEC=$((3 + 4 + 2))
WORKING_INFO_EFFECTIVE_SEC=$((((WORKING_INFO_PERIOD_SEC + MAIN_LOOP_QUANTUM_SEC - 1) / MAIN_LOOP_QUANTUM_SEC) * MAIN_LOOP_QUANTUM_SEC))
WORKING_INFO_STARTUP_LATENCY_SEC=$((MAIN_LOOP_QUANTUM_SEC * 2))
MIN_STEADY_STATE_WINDOW_SEC=$((WORKING_INFO_STARTUP_LATENCY_SEC + (MIN_VALID_STREAK - 1) * WORKING_INFO_EFFECTIVE_SEC))
MIN_DURATION_DIAG_SEC=$((REPOWER_OFF_WINDOW_SEC + MIN_CHARGE_SETTLE_SEC + MIN_STEADY_STATE_WINDOW_SEC))
ROM_FLASH_BLOCK_COUNT=$(((ROM_FLASH_IMAGE_BYTES + ROM_FLASH_BLOCK_BYTES - 1) / ROM_FLASH_BLOCK_BYTES))
ROM_FLASH_WIRE_MS=$(((ROM_FLASH_BLOCK_COUNT * ROM_FLASH_BLOCK_ONWIRE_BYTES * ROM_FLASH_BITS_PER_BYTE * 1000 + I2C_SLOW_BUS_BPS - 1) / I2C_SLOW_BUS_BPS))
ROM_FLASH_GAP_MS=$((ROM_FLASH_BLOCK_COUNT * ROM_FLASH_WRITE_GAP_MS))
ROM_FLASH_TRANSFER_SEC=$((((ROM_FLASH_WIRE_MS + ROM_FLASH_GAP_MS) + 999) / 1000))
MIN_DURATION_RECOVER_SEC=$((MIN_DURATION_DIAG_SEC + POST_FLASH_BOOT_QUIET_SEC + POST_FLASH_RESUME_WINDOW_SEC + ROM_FLASH_TRANSFER_SEC + ROM_FLASH_FIXED_LATENCY_SEC))

usage() {
  cat <<USAGE
Usage:
  $(basename "$0") diagnose [--mode canonical|dual-diag] [--duration-sec N] [--flash true|false] [--force-min-charge true|false] [--probe-mode strict|mac-only] [--rom-image r2|r3|r5] [--monitor-file PATH] [--report-out DIR]
  $(basename "$0") recover  [--mode dual-diag] [--duration-sec N] [--flash true|false] [--recover never|if-rom|force] [--force-min-charge true|false] [--probe-mode strict|mac-only] [--rom-image r2|r3|r5] [--monitor-file PATH] [--report-out DIR]
  $(basename "$0") verify   --monitor-file PATH [--mode canonical|dual-diag] [--duration-sec N] [--report-out DIR]
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
      mode_arg_set="true"
      shift 2
      ;;
    --duration-sec)
      require_value "$1" "$#"
      duration_sec="${2:-}"
      duration_arg_set="true"
      shift 2
      ;;
    --flash)
      require_value "$1" "$#"
      flash="${2:-}"
      flash_arg_set="true"
      shift 2
      ;;
    --recover)
      require_value "$1" "$#"
      recover_policy="${2:-}"
      recover_arg_set="true"
      shift 2
      ;;
    --force-min-charge)
      require_value "$1" "$#"
      force_min_charge="${2:-}"
      force_min_charge_arg_set="true"
      shift 2
      ;;
    --probe-mode)
      require_value "$1" "$#"
      probe_mode="${2:-}"
      probe_mode_arg_set="true"
      shift 2
      ;;
    --rom-image)
      require_value "$1" "$#"
      rom_image="${2:-}"
      rom_image_arg_set="true"
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

case "$subcommand" in
  help)
    usage
    exit 0
    ;;
  diagnose)
    if [[ "$recover_arg_set" == "true" ]]; then
      echo "diagnose mode does not accept --recover" >&2
      exit 11
    fi
    recover_policy="never"
    ;;
  recover)
    if [[ "$mode_arg_set" != "true" ]]; then
      mode="dual-diag"
    fi
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

if [[ "$subcommand" == "recover" && "$mode" != "dual-diag" ]]; then
  echo "recover requires --mode dual-diag" >&2
  exit 18
fi

if [[ "$subcommand" == "recover" && "$duration_arg_set" != "true" ]]; then
  duration_sec="$MIN_DURATION_RECOVER_SEC"
fi

if ! [[ "$duration_sec" =~ ^[0-9]+$ ]] || [[ "$duration_sec" -lt 1 ]]; then
  echo "Invalid --duration-sec: $duration_sec" >&2
  exit 5
fi

if [[ "$subcommand" != "verify" ]]; then
  min_duration_sec="$MIN_DURATION_DIAG_SEC"
  if [[ "$subcommand" == "recover" ]]; then
    min_duration_sec="$MIN_DURATION_RECOVER_SEC"
  fi
  if [[ "$duration_sec" -lt "$min_duration_sec" ]]; then
    if [[ "$subcommand" == "recover" ]]; then
      echo "duration-sec must be >= $min_duration_sec for recover (10s repower-off + 2s min-charge settle + ${WORKING_INFO_STARTUP_LATENCY_SEC}s startup-to-first-sample + streak>=${MIN_VALID_STREAK} at ~${WORKING_INFO_EFFECTIVE_SEC}s effective working-info cadence on a ${MAIN_LOOP_QUANTUM_SEC}s loop + ${POST_FLASH_BOOT_QUIET_SEC}s post-flash boot quiet + ${POST_FLASH_RESUME_WINDOW_SEC}s post-flash resume window + current ROM flash lower-bound ${ROM_FLASH_TRANSFER_SEC}s transfer/gap budget + ${ROM_FLASH_FIXED_LATENCY_SEC}s erase/execute/dwell)" >&2
    else
      echo "duration-sec must be >= $min_duration_sec for $subcommand (10s repower-off + 2s min-charge settle + ${WORKING_INFO_STARTUP_LATENCY_SEC}s startup-to-first-sample + streak>=${MIN_VALID_STREAK} at ~${WORKING_INFO_EFFECTIVE_SEC}s effective working-info cadence on a ${MAIN_LOOP_QUANTUM_SEC}s loop)" >&2
    fi
    exit 14
  fi
fi

if [[ "$subcommand" == "verify" ]]; then
  if [[ -z "$monitor_file" ]]; then
    echo "verify mode requires --monitor-file" >&2
    exit 8
  fi
  if [[ "$flash_arg_set" == "true" || "$recover_arg_set" == "true" || "$force_min_charge_arg_set" == "true" || "$probe_mode_arg_set" == "true" || "$rom_image_arg_set" == "true" ]]; then
    echo "verify mode does not accept --flash, --recover, --force-min-charge, --probe-mode, or --rom-image" >&2
    exit 10
  fi
else
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

  case "$force_min_charge" in
    true|false) ;;
    *)
      echo "Invalid --force-min-charge: $force_min_charge" >&2
      exit 15
      ;;
  esac

  case "$probe_mode" in
    strict|mac-only) ;;
    *)
      echo "Invalid --probe-mode: $probe_mode" >&2
      exit 16
      ;;
  esac

  case "$rom_image" in
    r2|r3|r5) ;;
    *)
      echo "Invalid --rom-image: $rom_image" >&2
      exit 17
      ;;
  esac

  if [[ "$recover_policy" == "force" && "$mode" != "dual-diag" ]]; then
    echo "--recover force requires --mode dual-diag" >&2
    exit 9
  fi

  "$SCRIPT_DIR/build.sh" --mode "$mode" --recover "$recover_policy" --force-min-charge "$force_min_charge" --probe-mode "$probe_mode" --rom-image "$rom_image"

  if [[ "$flash" == "true" ]]; then
    "$SCRIPT_DIR/flash.sh"
  fi

  monitor_reset_on_attach="true"
  if [[ "$flash" == "true" ]]; then
    # A fresh flash already rebooted the MCU, so let monitor.sh try a clean attach first.
    monitor_reset_on_attach="false"
  fi
  monitor_args=(--duration-sec "$duration_sec" --after-flash "$flash" --reset-on-attach "$monitor_reset_on_attach")
  if [[ -n "$monitor_file" ]]; then
    monitor_args+=(--output "$monitor_file")
  fi
  monitor_file=$("$SCRIPT_DIR/monitor.sh" "${monitor_args[@]}")
fi

if [[ ! -f "$monitor_file" ]]; then
  echo "monitor file not found: $monitor_file" >&2
  exit 12
fi
if [[ ! -r "$monitor_file" ]]; then
  echo "monitor file is not readable: $monitor_file" >&2
  exit 13
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
