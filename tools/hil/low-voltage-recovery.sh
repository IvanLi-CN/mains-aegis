#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

TARGET_DEVICE_ID_DEFAULT="serial-04f3bb3f5367"
TARGET_PORT_DEFAULT="/dev/cu.usbmodem212301"
DENIED_PORT="/dev/cu.usbmodem212101"

mode="dry-run"
target_device_id=""
target_port=""
devd_url="http://127.0.0.1:30080"
report_root=""
require_recovery_state="false"
skip_main_build="false"

usage() {
  cat <<USAGE
Usage:
  $(basename "$0") [--dry-run]
  $(basename "$0") --real --device-id serial-04f3bb3f5367 --port /dev/cu.usbmodem212301 [options]

Options:
  --devd-url URL                 mains-aegis-devd URL (default: $devd_url)
  --report-root DIR              HIL report root (default: tools/hil/reports/<timestamp>)
  --require-recovery-state true  require power-diag policy.status=RECOV
  --skip-main-build true         reuse an existing release main firmware ELF

This runner performs the controlled two-flash HIL flow:
  1. flash bq40-comm-tool firmware and apply live-df-mainboard
  2. flash main firmware through mains-aegis-devd
  3. verify USB power-diag exposes the recovery baseline
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
    --dry-run)
      mode="dry-run"
      shift
      ;;
    --real)
      mode="real"
      shift
      ;;
    --device-id)
      require_value "$1" "$#"
      target_device_id="${2:-}"
      shift 2
      ;;
    --port)
      require_value "$1" "$#"
      target_port="${2:-}"
      shift 2
      ;;
    --devd-url)
      require_value "$1" "$#"
      devd_url="${2:-}"
      shift 2
      ;;
    --report-root)
      require_value "$1" "$#"
      report_root="${2:-}"
      shift 2
      ;;
    --require-recovery-state)
      require_value "$1" "$#"
      require_recovery_state="${2:-}"
      shift 2
      ;;
    --skip-main-build)
      require_value "$1" "$#"
      skip_main_build="${2:-}"
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

case "$require_recovery_state" in
  true|false) ;;
  *)
    echo "Invalid --require-recovery-state: $require_recovery_state" >&2
    exit 2
    ;;
esac

case "$skip_main_build" in
  true|false) ;;
  *)
    echo "Invalid --skip-main-build: $skip_main_build" >&2
    exit 2
    ;;
esac

timestamp=$(date -u +"%Y%m%dT%H%M%SZ")
if [[ -z "$report_root" ]]; then
  report_root="$REPO_ROOT/tools/hil/reports/$timestamp"
fi
mkdir -p "$report_root"

target_device_id="${target_device_id:-$TARGET_DEVICE_ID_DEFAULT}"
target_port="${target_port:-$TARGET_PORT_DEFAULT}"

decision_summary() {
  local op_type="$1"
  local command="$2"
  local decision="$3"
  local rationale="$4"
  local next_step="$5"
  printf 'Operation type: %s\nCommand: %s\nDecision: %s\nRationale: %s\nNext step: %s\n\n' \
    "$op_type" "$command" "$decision" "$rationale" "$next_step"
}

run_step() {
  local desc="$1"
  shift
  echo "==> $desc"
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if [[ "$mode" == "real" ]]; then
    "$@"
  fi
}

read_selector_port() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    return 1
  fi
  awk 'NF && $0 !~ /^[[:space:]]*#/ && $0 !~ /^mac=/ { print; exit }' "$path"
}

require_exact_target() {
  if [[ "$target_device_id" != "$TARGET_DEVICE_ID_DEFAULT" ]]; then
    decision_summary "state-changing" "$0 --real" "deny" "G5: target device id is not the approved HIL device" "Use $TARGET_DEVICE_ID_DEFAULT or stop."
    exit 32
  fi
  if [[ "$target_port" != "$TARGET_PORT_DEFAULT" ]]; then
    decision_summary "state-changing" "$0 --real" "deny" "G5: target port is not the approved HIL port" "Use $TARGET_PORT_DEFAULT or stop."
    exit 33
  fi
  if [[ "$target_port" == "$DENIED_PORT" ]]; then
    decision_summary "state-changing" "$0 --real" "deny" "G3: denied port must never be used or tried" "Stop and rebind to the approved target."
    exit 34
  fi
}

validate_selector_cache() {
  local label="$1"
  local path="$2"
  local selected=""
  selected=$(read_selector_port "$path" || true)
  if [[ -z "$selected" ]]; then
    decision_summary "read-only" "read $path" "deny" "G5: no known bound port for $label" "Bind the approved target before real HIL."
    exit 40
  fi
  if [[ "$selected" == "$DENIED_PORT" ]]; then
    decision_summary "read-only" "read $path" "deny" "G3: $label is bound to denied port $DENIED_PORT" "Do not flash; rebind to $TARGET_PORT_DEFAULT."
    exit 41
  fi
  if [[ "$selected" != "$target_port" ]]; then
    decision_summary "read-only" "read $path" "deny" "G5: $label binding $selected does not match approved target $target_port" "Do not flash; fix the binding explicitly."
    exit 42
  fi
}

python_json() {
  python3 - "$@"
}

curl_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  if [[ -n "$body" ]]; then
    curl -fsS -X "$method" -H 'content-type: application/json' --data "$body" "$url"
  else
    curl -fsS -X "$method" "$url"
  fi
}

ensure_devd() {
  if curl -fsS "$devd_url/health" >/dev/null 2>&1; then
    echo "Using existing mains-aegis-devd at $devd_url"
    return 0
  fi
  local bind="${devd_url#http://}"
  if [[ "$bind" == "$devd_url" || "$bind" == */* ]]; then
    echo "Cannot auto-start devd for URL: $devd_url" >&2
    exit 50
  fi
  echo "Starting mains-aegis-devd at $devd_url"
  (cd "$REPO_ROOT" && cargo run --manifest-path tools/mains-aegis-devd/Cargo.toml -- serve --bind "$bind") \
    >"$report_root/devd.log" 2>&1 &
  DEVD_PID=$!
  trap 'if [[ -n "${DEVD_PID:-}" ]]; then kill "$DEVD_PID" 2>/dev/null || true; fi' EXIT
  for _ in $(seq 1 40); do
    if curl -fsS "$devd_url/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "mains-aegis-devd did not become healthy; see $report_root/devd.log" >&2
  exit 51
}

validate_scan_target() {
  local scan_json="$1"
  python_json "$scan_json" "$target_device_id" "$target_port" <<'PY'
import json, sys
payload = json.loads(sys.argv[1])
target_id = sys.argv[2]
target_port = sys.argv[3]
devices = payload.get("devices", [])
matches = [d for d in devices if d.get("id") == target_id]
if len(matches) != 1:
    raise SystemExit(f"target device {target_id} not uniquely present in devd scan")
port = matches[0].get("port_path")
if port != target_port:
    raise SystemExit(f"target device port mismatch: {port!r} != {target_port!r}")
PY
}

find_manifest() {
  local artifact_dir="$1"
  python_json "$artifact_dir" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
matches = sorted(root.glob("*.manifest.json"))
if len(matches) != 1:
    raise SystemExit(f"expected exactly one manifest in {root}, found {len(matches)}")
print(matches[0])
PY
}

validate_power_diag() {
  local diag_json="$1"
  local out="$2"
  python_json "$diag_json" "$require_recovery_state" "$out" <<'PY'
import json, sys
diag = json.loads(sys.argv[1])
require_recovery = sys.argv[2] == "true"
out = sys.argv[3]
charger = diag.get("charger", {})
bms = diag.get("bms", {})
policy = diag.get("policy", {})
errors = []
if charger.get("vbat_lowv_pct_x10") != 714:
    errors.append(f"charger.vbat_lowv_pct_x10={charger.get('vbat_lowv_pct_x10')!r}, expected 714")
if charger.get("iprechg_ma") != 120:
    errors.append(f"charger.iprechg_ma={charger.get('iprechg_ma')!r}, expected 120")
if bms.get("cuv_recovery_mv") != 2900:
    errors.append(f"bms.cuv_recovery_mv={bms.get('cuv_recovery_mv')!r}, expected 2900")
if bms.get("cuv_recov_chg") is not True:
    errors.append(f"bms.cuv_recov_chg={bms.get('cuv_recov_chg')!r}, expected true")
stage = policy.get("recovery_stage")
if stage not in (None, "bq40_pchg", "bq25792_precharge"):
    errors.append(f"policy.recovery_stage={stage!r}, expected null/bq40_pchg/bq25792_precharge")
if require_recovery:
    if policy.get("status") != "RECOV":
        errors.append(f"policy.status={policy.get('status')!r}, expected RECOV")
    if stage not in ("bq40_pchg", "bq25792_precharge"):
        errors.append(f"policy.recovery_stage={stage!r}, expected active recovery stage")
Path = __import__("pathlib").Path
Path(out).write_text(json.dumps({"ok": not errors, "errors": errors, "diag": diag}, indent=2, sort_keys=True) + "\n")
if errors:
    raise SystemExit("\n".join(errors))
PY
}

main_elf="$REPO_ROOT/firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware"
main_artifact_dir="$report_root/main-artifact"

if [[ "$mode" == "real" ]]; then
  require_exact_target
  validate_selector_cache "main firmware" "$REPO_ROOT/firmware/.esp32-port"
  validate_selector_cache "bq40-comm-tool" "$REPO_ROOT/tools/bq40-comm-tool/.esp32-port"
  decision_summary "state-changing" "tools/bq40-comm-tool/bin/run.sh apply-df ... && devd flash ..." "allow" "G4/G5: known approved bound device; no direct espflash, no port enumeration/switching, devd scan is owner-visible" "Run two-flash HIL."
else
  echo "Dry-run mode: no hardware flash, monitor, reset, scan, or USB request will be executed."
fi

if [[ ! -x "$REPO_ROOT/tools/bq40-comm-tool/bin/run.sh" ]]; then
  echo "bq40-comm-tool runner is missing or not executable" >&2
  exit 60
fi
echo "Validated bq40-comm-tool runner."

run_step "Apply BQ40 live DF baseline through temporary tool firmware" \
  "$REPO_ROOT/tools/bq40-comm-tool/bin/run.sh" apply-df \
  --mode canonical \
  --duration-sec 120 \
  --force-min-charge true \
  --repair-profile live-df-mainboard \
  --report-out "$report_root/bq40-apply-df"

if [[ "$skip_main_build" != "true" ]]; then
  run_step "Build main firmware release" \
    bash -lc "cd '$REPO_ROOT/firmware' && cargo build --release --bin esp-firmware --features net_http,web_serial"
fi

if [[ "$mode" == "real" && ! -f "$main_elf" ]]; then
  echo "main firmware ELF not found: $main_elf" >&2
  exit 61
fi
if [[ "$mode" != "real" && ! -f "$main_elf" ]]; then
  echo "Dry-run note: main firmware ELF is not present yet; real HIL will build it unless --skip-main-build true is used."
else
  echo "Validated main firmware ELF path: $main_elf"
fi

run_step "Generate main firmware catalog manifest" \
  python3 "$REPO_ROOT/tools/firmware-artifact/build-catalog-entry.py" \
  --elf "$main_elf" \
  --out "$main_artifact_dir" \
  --features net_http,web_serial

if [[ "$mode" != "real" ]]; then
  echo "Dry-run completed. Real HIL would now start devd, scan the approved device, flash main firmware, and validate power-diag."
  exit 0
fi

manifest_path=$(find_manifest "$main_artifact_dir")
ensure_devd

scan_json=$(curl_json POST "$devd_url/api/v1/devices/scan" '{}')
printf '%s\n' "$scan_json" > "$report_root/devd-scan.json"
validate_scan_target "$scan_json"

curl_json POST "$devd_url/api/v1/devices/$target_device_id/bind" '{"alias":"hil-low-voltage-recovery"}' \
  > "$report_root/devd-bind.json"
curl_json POST "$devd_url/api/v1/devices/$target_device_id/artifact" "{\"manifest_path\":\"$manifest_path\"}" \
  > "$report_root/devd-artifact.json"
curl_json POST "$devd_url/api/v1/devices/$target_device_id/flash" '{"dry_run":false}' \
  > "$report_root/devd-main-flash.json"

sleep 8
curl_json POST "$devd_url/api/v1/devices/$target_device_id/connect" '{}' \
  > "$report_root/devd-connect.json"
diag_json=$(curl_json GET "$devd_url/api/v1/devices/$target_device_id/power-diag")
printf '%s\n' "$diag_json" > "$report_root/power-diag.json"
validate_power_diag "$diag_json" "$report_root/hil-result.json"

echo "HIL completed: $report_root/hil-result.json"
