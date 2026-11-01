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

for required in \
  "bms-df-repair-afe-default" \
  "bms-rom-repair-asset-df-mainboard" \
  "bms-rom-image-r3"; do
  if ! rg -q "$required" "$SCRIPT_DIR/build.sh"; then
    echo "recovery feature path missing from build.sh: $required" >&2
    exit 1
  fi
done

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

echo "bq40 profile shell contract: ok"
