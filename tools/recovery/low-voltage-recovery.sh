#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

mode="dry-run"
target_device_id=""
target_port=""
target_device_id_arg_set="false"
target_port_arg_set="false"
devd_url="http://127.0.0.1:30080"
report_root=""
require_recovery_state="false"
skip_main_build="false"

usage() {
  cat <<USAGE
Usage:
  $(basename "$0") [--dry-run]
  $(basename "$0") --real --device-id <devd-device-id> --port <serial-port> [options]

Options:
  --devd-url URL                 mains-aegis-devd URL (default: $devd_url)
  --report-root DIR              Recovery report root (default: tools/recovery/reports/<timestamp>)
  --require-recovery-state true  require diag-snapshot policy.status=RECOV
  --skip-main-build true         reuse an existing release main firmware ELF

This runner performs the controlled two-flash recovery maintenance flow:
  1. flash bq40-comm-tool firmware and apply live-df-mainboard
  2. flash main firmware through mains-aegis-devd
  3. verify USB diag-snapshot exposes the recovery baseline
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
      target_device_id_arg_set="true"
      shift 2
      ;;
    --port)
      require_value "$1" "$#"
      target_port="${2:-}"
      target_port_arg_set="true"
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
  report_root="$REPO_ROOT/tools/recovery/reports/$timestamp"
fi
mkdir -p "$report_root"

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

require_explicit_target() {
  if [[ "$target_device_id_arg_set" != "true" || -z "$target_device_id" || "$target_port_arg_set" != "true" || -z "$target_port" ]]; then
    decision_summary "state-changing" "$0 --real" "deny" "G5: recovery maintenance requires explicit device id and port for this invocation" "Pass --device-id <devd-device-id> --port <serial-port>."
    exit 31
  fi
}

validate_selector_cache() {
  local label="$1"
  local path="$2"
  local selected=""
  selected=$(read_selector_port "$path" || true)
  if [[ -z "$selected" ]]; then
    decision_summary "read-only" "read $path" "deny" "G5: no known bound port for $label" "Bind the explicit target before recovery maintenance."
    exit 40
  fi
  if [[ "$selected" != "$target_port" ]]; then
    decision_summary "read-only" "read $path" "deny" "G5: $label binding $selected does not match explicit target $target_port" "Do not flash; fix the binding explicitly."
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
  echo "mains-aegis-devd is not healthy at $devd_url" >&2
  echo "Start it with: just devd-http" >&2
  exit 51
}

validate_scan_target() {
  local scan_json="$1"
  local scan_file
  scan_file=$(mktemp)
  printf '%s\n' "$scan_json" > "$scan_file"
  python3 - "$scan_file" "$target_device_id" "$target_port" <<'PY'
import json, sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text())
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
  rm -f "$scan_file"
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

validate_diag_snapshot() {
  local diag_json="$1"
  local out="$2"
  python_json "$diag_json" "$require_recovery_state" "$out" <<'PY'
import json, sys
diag = json.loads(sys.argv[1])
packages = diag.get("packages", {})
if isinstance(packages, dict):
    derived = packages.get("derived.power", {})
    if isinstance(derived, dict) and isinstance(derived.get("payload"), dict):
        diag = derived["payload"]
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
if bms.get("cuv_recovery_mv") != 2550:
    errors.append(f"bms.cuv_recovery_mv={bms.get('cuv_recovery_mv')!r}, expected 2550")
if bms.get("cuv_recov_chg") is not False:
    errors.append(f"bms.cuv_recov_chg={bms.get('cuv_recov_chg')!r}, expected false")
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

poll_diag_snapshot() {
  local out="$1"
  local result_out="$2"
  local timeout_sec=45
  local interval_sec=5
  local elapsed=0
  local attempt=0
  local diag_json=""

  while (( elapsed <= timeout_sec )); do
    attempt=$((attempt + 1))
    diag_json=$(curl_json GET "$devd_url/api/v1/devices/$target_device_id/diag-snapshot?package=bq25792.regs&package=bq40.manufacturing&package=derived.power")
    printf '%s\n' "$diag_json" > "$report_root/diag-snapshot-attempt-$attempt.json"
    printf '%s\n' "$diag_json" > "$out"
    if validate_diag_snapshot "$diag_json" "$result_out"; then
      echo "Validated diag-snapshot after ${elapsed}s."
      return 0
    fi
    if (( elapsed == timeout_sec )); then
      break
    fi
    sleep "$interval_sec"
    elapsed=$((elapsed + interval_sec))
  done

  echo "diag-snapshot did not reach the expected recovery state within ${timeout_sec}s" >&2
  validate_diag_snapshot "$diag_json" "$result_out"
}

main_elf="$REPO_ROOT/firmware/target/xtensa-esp32s3-none-elf/release/mains-aegis-firmware"
main_artifact_dir="$report_root/main-artifact"

if [[ "$mode" == "real" ]]; then
  require_explicit_target
  validate_selector_cache "main firmware" "$REPO_ROOT/firmware/.esp32-port"
  validate_selector_cache "bq40-comm-tool" "$REPO_ROOT/tools/bq40-comm-tool/.esp32-port"
  ensure_devd
  scan_json=$(curl_json POST "$devd_url/api/v1/devices/scan" '{}')
  printf '%s\n' "$scan_json" > "$report_root/devd-scan-pre-bq40.json"
  validate_scan_target "$scan_json"
  curl_json POST "$devd_url/api/v1/devices/$target_device_id/bind" '{"alias":"low-voltage-recovery"}' \
    > "$report_root/devd-bind-pre-bq40.json"
  if connect_json=$(curl_json POST "$devd_url/api/v1/devices/$target_device_id/connect" '{}' 2>/dev/null); then
    printf '%s\n' "$connect_json" > "$report_root/devd-connect-pre-bq40.json"
  else
    echo "Bound device is present but identity is unavailable; continuing from download mode." >&2
  fi
  decision_summary "state-changing" "tools/bq40-comm-tool/bin/run.sh apply-df ... && devd flash ..." "allow" "G4/G5: explicit owner-supplied device id and port; devd scan confirmed the same port; no direct espflash and no port switching" "Run two-flash recovery maintenance."
else
  echo "Dry-run mode: no hardware flash, monitor, reset, scan, or USB request will be executed."
fi

if [[ ! -x "$REPO_ROOT/tools/bq40-comm-tool/bin/run.sh" ]]; then
  echo "bq40-comm-tool runner is missing or not executable" >&2
  exit 60
fi
echo "Validated bq40-comm-tool runner."

run_step "Apply BQ40 live DF baseline through temporary tool firmware" \
  env \
    BQ40_TOOL_DEVD_URL="$devd_url" \
    BQ40_TOOL_DEVICE_ID="$target_device_id" \
    "$REPO_ROOT/tools/bq40-comm-tool/bin/run.sh" apply-df \
    --mode canonical \
    --duration-sec 120 \
    --force-min-charge true \
    --repair-profile live-df-mainboard \
    --report-out "$report_root/bq40-apply-df"

if [[ "$skip_main_build" != "true" ]]; then
  run_step "Build main firmware release" \
    bash -lc "cd '$REPO_ROOT' && just firmware-build"
fi

if [[ "$mode" == "real" && ! -f "$main_elf" ]]; then
  echo "main firmware ELF not found: $main_elf" >&2
  exit 61
fi
if [[ "$mode" != "real" && ! -f "$main_elf" ]]; then
  echo "Dry-run note: main firmware ELF is not present yet; real recovery maintenance will build it unless --skip-main-build true is used."
else
  echo "Validated main firmware ELF path: $main_elf"
fi

run_step "Generate main firmware catalog manifest" \
  python3 "$REPO_ROOT/tools/firmware-artifact/build-catalog-entry.py" \
  --elf "$main_elf" \
  --out "$main_artifact_dir" \
  --features net_http,web_serial

if [[ "$mode" != "real" ]]; then
  echo "Dry-run completed. Real recovery maintenance would now require an explicit device id and port, start devd, scan that target, flash main firmware, and validate diag-snapshot."
  exit 0
fi

manifest_path=$(find_manifest "$main_artifact_dir")
ensure_devd

scan_json=$(curl_json POST "$devd_url/api/v1/devices/scan" '{}')
printf '%s\n' "$scan_json" > "$report_root/devd-scan.json"
validate_scan_target "$scan_json"

curl_json POST "$devd_url/api/v1/devices/$target_device_id/bind" '{"alias":"low-voltage-recovery"}' \
  > "$report_root/devd-bind.json"
curl_json POST "$devd_url/api/v1/devices/$target_device_id/artifact" "{\"manifest_path\":\"$manifest_path\"}" \
  > "$report_root/devd-artifact.json"
curl_json POST "$devd_url/api/v1/devices/$target_device_id/flash" '{"dry_run":false}' \
  > "$report_root/devd-main-flash.json"

sleep 8
if connect_json=$(curl_json POST "$devd_url/api/v1/devices/$target_device_id/connect" '{}' 2>/dev/null); then
  printf '%s\n' "$connect_json" > "$report_root/devd-connect.json"
else
  echo "Bound device is present but identity is unavailable after main flash; continuing to diag-snapshot poll." >&2
fi
poll_diag_snapshot "$report_root/diag-snapshot.json" "$report_root/recovery-result.json"

echo "Recovery maintenance completed: $report_root/recovery-result.json"
