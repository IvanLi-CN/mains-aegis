#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$TOOL_ROOT/../.." && pwd)

expect_exit_with_text() {
  local expected_status="$1"
  local expected_text="$2"
  shift 2

  local output
  local status
  set +e
  output=$("$@" 2>&1)
  status=$?
  set -e

  if [[ "$status" != "$expected_status" ]]; then
    echo "expected exit $expected_status, got $status: $*" >&2
    printf '%s\n' "$output" >&2
    exit 1
  fi
  if [[ "$output" != *"$expected_text"* ]]; then
    echo "expected output to contain '$expected_text': $*" >&2
    printf '%s\n' "$output" >&2
    exit 1
  fi
}

expect_exit_with_text 23 \
  "live-df-mainboard was retired" \
  "$SCRIPT_DIR/build.sh" --repair-profile live-df-mainboard

expect_exit_with_text 23 \
  "live-df-mainboard was retired" \
  "$SCRIPT_DIR/run.sh" apply-df --repair-profile live-df-mainboard

expect_exit_with_text 20 \
  "apply-df requires --repair-profile bq40-charge-temperature" \
  "$SCRIPT_DIR/run.sh" apply-df --repair-profile none

expect_exit_with_text 21 \
  "cannot be combined with --recover if-rom" \
  "$SCRIPT_DIR/build.sh" --mode dual-diag --recover if-rom --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires the strict identity probe" \
  "$SCRIPT_DIR/build.sh" --probe-mode mac-only --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires the strict identity probe" \
  "$SCRIPT_DIR/run.sh" apply-df --probe-mode mac-only --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires canonical mode" \
  "$SCRIPT_DIR/build.sh" --mode dual-diag --recover never --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires canonical mode" \
  "$SCRIPT_DIR/run.sh" apply-df --mode dual-diag --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires apply-df or verify action" \
  "$SCRIPT_DIR/report.sh" --action diagnose --monitor-file /tmp/mains-aegis-missing-charge-profile-log --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires apply-df or verify action" \
  "$SCRIPT_DIR/report.sh" --action recover --monitor-file /tmp/mains-aegis-missing-charge-profile-log --repair-profile bq40-charge-temperature

expect_exit_with_text 21 \
  "cannot be combined with --rom-image r3" \
  "$SCRIPT_DIR/build.sh" --rom-image r3 --repair-profile bq40-charge-temperature

expect_exit_with_text 12 \
  "monitor file not found" \
  "$SCRIPT_DIR/run.sh" verify --monitor-file /tmp/mains-aegis-missing-charge-profile-log --repair-profile bq40-charge-temperature

expect_exit_with_text 24 \
  "requires canonical mode" \
  "$SCRIPT_DIR/run.sh" verify --mode dual-diag --monitor-file /tmp/mains-aegis-missing-charge-profile-log --repair-profile bq40-charge-temperature

expect_exit_with_text 17 \
  "Invalid --repair-profile" \
  "$SCRIPT_DIR/run.sh" verify --monitor-file /tmp/mains-aegis-missing-charge-profile-log --repair-profile asset-df-mainboard

for required_path in \
  "$REPO_ROOT/firmware/src" \
  "$REPO_ROOT/web/src" \
  "$TOOL_ROOT/firmware/src"; do
  if [[ ! -d "$required_path" ]]; then
    echo "required contract path is missing: $required_path" >&2
    exit 1
  fi
done

for required in \
  "bms-rom-recover-force" \
  "bms-df-repair-afe-default" \
  "bms-rom-repair-asset-df-mainboard" \
  "bms-rom-image-r3"; do
  if ! rg -q "$required" "$SCRIPT_DIR/build.sh"; then
    echo "recovery feature path missing from build.sh: $required" >&2
    exit 1
  fi
done

if ! rg -q 'bms-charge-temperature-profile requires the strict identity probe and cannot be combined with mac-only, BQ40 recovery, ROM image, or repair features' \
  "$TOOL_ROOT/firmware/src/output/mod.rs"; then
  echo "profile/recovery compile-time conflict guard is missing" >&2
  exit 1
fi

if profile_only_output=$(
  cd "$TOOL_ROOT/firmware"
  cargo +esp check --features bms-charge-temperature-profile 2>&1
); then
  echo "profile-only build unexpectedly compiled without bms-rom-recover-disable" >&2
  printf '%s\n' "$profile_only_output" >&2
  exit 1
fi
if [[ "$profile_only_output" != *"requires bms-rom-recover-disable"* ]]; then
  echo "profile-only build did not enforce the ROM-recovery disable guard" >&2
  printf '%s\n' "$profile_only_output" >&2
  exit 1
fi

if incompatible_output=$(
  cd "$TOOL_ROOT/firmware"
  cargo +esp check --features bms-charge-temperature-profile,bms-rom-recover-force 2>&1
); then
  echo "profile/recovery feature combination unexpectedly compiled" >&2
  printf '%s\n' "$incompatible_output" >&2
  exit 1
fi
if [[ "$incompatible_output" != *"bms-charge-temperature-profile requires the strict identity probe"* ]]; then
  echo "profile/recovery compile-time conflict was not enforced by cargo" >&2
  printf '%s\n' "$incompatible_output" >&2
  exit 1
fi

if incompatible_output=$(
  cd "$TOOL_ROOT/firmware"
  cargo +esp check --features bms-charge-temperature-profile,bms-rom-image-r3 2>&1
); then
  echo "profile/ROM-image feature combination unexpectedly compiled" >&2
  printf '%s\n' "$incompatible_output" >&2
  exit 1
fi
if [[ "$incompatible_output" != *"cannot be combined with mac-only, BQ40 recovery, ROM image, or repair features"* ]]; then
  echo "profile/ROM-image compile-time conflict was not enforced by cargo" >&2
  printf '%s\n' "$incompatible_output" >&2
  exit 1
fi

python3 "$SCRIPT_DIR/report-parser-test.py"

if rg -n "Ma1000|ManualSpeed1A|CHG1A" \
  "$REPO_ROOT/firmware/src" \
  "$REPO_ROOT/web/src" \
  "$TOOL_ROOT/firmware/src"; then
  echo "retired 1A ordinary UI/USB path is still present" >&2
  exit 1
fi

if rg -n "ma_1000" \
  --glob '!usb_cdc_protocol.rs' \
  "$REPO_ROOT/firmware/src" \
  "$REPO_ROOT/web/src" \
  "$TOOL_ROOT/firmware/src"; then
  echo "retired 1A ordinary UI/USB value is still present outside its rejection test" >&2
  exit 1
fi

poll_writer_refs=$(awk '
  /^    fn maybe_poll_bms/ { in_poll = 1; next }
  in_poll && /^    fn / { exit }
  in_poll && /maybe_apply_bq40_charge_temperature_profile/ { print }
' "$TOOL_ROOT/firmware/src/output/mod.rs")
if [[ -n "$poll_writer_refs" ]]; then
  echo "charge-temperature Data Flash writer is still reachable from ordinary BMS polling" >&2
  printf '%s\n' "$poll_writer_refs" >&2
  exit 1
fi

profile_transport=$(awk '
  /struct Bq40ChargeTemperatureTransport/ { in_profile = 1 }
  in_profile && /^fn read_bms_block_u32/ { exit }
  in_profile { print }
' "$TOOL_ROOT/firmware/src/output/mod.rs")
if [[ "$profile_transport" == *"read_bms_df_block_via_mb44"* ]]; then
  echo "charge-temperature profile transport still uses retrying MB44 block reads" >&2
  exit 1
fi

echo "bq40 profile shell contract: ok"
