#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/common.sh"

bq40_tool_acquire_flash_monitor_lock "$TOOL_ROOT"

devd_url="${BQ40_TOOL_DEVD_URL:-http://127.0.0.1:30080}"
device_id="${BQ40_TOOL_DEVICE_ID:-}"
artifact_manifest_path="${BQ40_TOOL_ARTIFACT_MANIFEST_PATH:-}"

usage() {
  cat <<USAGE
Usage: $(basename "$0")
Environment:
  BQ40_TOOL_DEVD_URL               mains-aegis-devd URL (default: $devd_url)
  BQ40_TOOL_DEVICE_ID              approved devd device id
  BQ40_TOOL_ARTIFACT_MANIFEST_PATH manifest selected by bin/build.sh
USAGE
}

if [[ $# -gt 0 ]]; then
  case "$1" in
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
fi

if [[ -z "$device_id" ]]; then
  echo "BQ40_TOOL_DEVICE_ID is required for devd flash" >&2
  exit 3
fi

if [[ -z "$artifact_manifest_path" ]]; then
  echo "BQ40_TOOL_ARTIFACT_MANIFEST_PATH is required for devd flash" >&2
  exit 4
fi

if [[ ! -f "$artifact_manifest_path" ]]; then
  echo "artifact manifest not found: $artifact_manifest_path" >&2
  exit 5
fi

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

echo "Using mains-aegis-devd at $devd_url"
curl_json GET "$devd_url/health" >/dev/null

connect_json=$(curl_json POST "$devd_url/api/v1/devices/$device_id/connect" '{}')
printf '%s\n' "$connect_json"

artifact_json=$(curl_json POST "$devd_url/api/v1/devices/$device_id/artifact" "{\"manifest_path\":\"$artifact_manifest_path\"}")
printf '%s\n' "$artifact_json"

flash_json=$(curl_json POST "$devd_url/api/v1/devices/$device_id/flash" '{"dry_run":false}')
printf '%s\n' "$flash_json"
