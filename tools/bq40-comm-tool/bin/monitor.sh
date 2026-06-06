#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/common.sh"

bq40_tool_acquire_flash_monitor_lock "$TOOL_ROOT"

duration_sec=120
output_file=""
after_flash="false"
reset_on_attach="false"
initial_stdout_timeout_sec="6"
devd_url="${BQ40_TOOL_DEVD_URL:-http://127.0.0.1:30080}"
device_id="${BQ40_TOOL_DEVICE_ID:-}"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--duration-sec N] [--output PATH] [--after-flash true|false] [--reset-on-attach true|false] [--initial-stdout-timeout-sec N]
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
    --duration-sec)
      require_value "$1" "$#"
      duration_sec="${2:-}"
      shift 2
      ;;
    --output)
      require_value "$1" "$#"
      output_file="${2:-}"
      shift 2
      ;;
    --after-flash)
      require_value "$1" "$#"
      after_flash="${2:-}"
      shift 2
      ;;
    --reset-on-attach)
      require_value "$1" "$#"
      reset_on_attach="${2:-}"
      shift 2
      ;;
    --initial-stdout-timeout-sec)
      require_value "$1" "$#"
      initial_stdout_timeout_sec="${2:-}"
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

if ! [[ "$duration_sec" =~ ^[0-9]+$ ]] || [[ "$duration_sec" -lt 1 ]]; then
  echo "Invalid --duration-sec: $duration_sec" >&2
  exit 3
fi

case "$after_flash" in
  true|false) ;;
  *)
    echo "Invalid --after-flash: $after_flash" >&2
    exit 4
    ;;
esac

case "$reset_on_attach" in
  true|false) ;;
  *)
    echo "Invalid --reset-on-attach: $reset_on_attach" >&2
    exit 5
    ;;
esac

if ! [[ "$initial_stdout_timeout_sec" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "Invalid --initial-stdout-timeout-sec: $initial_stdout_timeout_sec" >&2
  exit 6
fi

if [[ -z "$device_id" ]]; then
  echo "BQ40_TOOL_DEVICE_ID is required for devd monitor" >&2
  exit 7
fi

if [[ -z "$output_file" ]]; then
  monitor_dir="$TOOL_ROOT/.devd-monitor/esp"
  mkdir -p "$monitor_dir"
  output_file=$(mktemp -p "$monitor_dir" "$(date +%Y%m%d_%H%M%S)_XXXXXX_combined.mon.ndjson")
fi

python3 - "$devd_url" "$device_id" "$duration_sec" "$after_flash" "$reset_on_attach" "$initial_stdout_timeout_sec" "$output_file" <<'PY'
from __future__ import annotations

import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

devd_url = sys.argv[1].rstrip("/")
device_id = sys.argv[2]
duration_sec = int(sys.argv[3])
after_flash = sys.argv[4] == "true"
reset_on_attach = sys.argv[5] == "true"
initial_stdout_timeout_sec = float(sys.argv[6])
output_path = Path(sys.argv[7]).expanduser().resolve()


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def fmt_ts(value: datetime) -> str:
    return value.isoformat().replace("+00:00", "Z")


def parse_ts(value: object) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def request_json(method: str, path: str, body: object | None = None) -> object:
    url = f"{devd_url}{path}"
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = Request(url, data=data, method=method)
    request.add_header("accept", "application/json")
    if data is not None:
        request.add_header("content-type", "application/json")
    try:
        with urlopen(request, timeout=max(30, duration_sec + 30)) as response:
            payload = response.read().decode("utf-8")
    except HTTPError as exc:
        payload = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"{method} {path} failed: {exc.code} {exc.reason}: {payload.strip()}") from exc
    except URLError as exc:
        raise SystemExit(f"{method} {path} failed: {exc.reason}") from exc
    return json.loads(payload)


def to_text(entry: dict[str, object]) -> str:
    for key in ("summary", "message", "payload"):
        value = entry.get(key)
        if isinstance(value, str) and value.strip():
            return value
    return ""


def entries_since(
    snapshot: dict[str, object],
    key: str,
    total_key: str,
    start_count: int,
    start_ts: datetime,
) -> list[object]:
    entries = snapshot.get(key, [])
    if not isinstance(entries, list):
        return []
    total = snapshot.get(total_key, len(entries))
    try:
        total_count = int(total or 0)
    except (TypeError, ValueError):
        total_count = len(entries)
    offset = max(0, total_count - len(entries))
    start_idx = max(0, start_count - offset)
    sliced = entries[start_idx:]
    if sliced or total_count > start_count:
        return sliced
    return [
        entry
        for entry in entries
        if isinstance(entry, dict)
        and (entry_ts := parse_ts(entry.get("timestamp"))) is not None
        and entry_ts >= start_ts
    ]


def has_device_activity_since(
    snapshot: dict[str, object],
    trace_start_count: int,
    log_start_count: int,
    start_ts: datetime,
) -> bool:
    for entry in entries_since(snapshot, "trace", "trace_count", trace_start_count, start_ts):
        if not isinstance(entry, dict):
            continue
        text = entry.get("text")
        if isinstance(text, str) and text.strip() and text.strip() != "get_status":
            return True
    for entry in entries_since(snapshot, "logs", "log_count", log_start_count, start_ts):
        if not isinstance(entry, dict):
            continue
        target = entry.get("target")
        message = entry.get("message")
        if target in (None, "monitor"):
            continue
        if isinstance(message, str) and message.strip():
            return True
    return False


print(f"Using mains-aegis-devd at {devd_url}", file=sys.stderr)
print(json.dumps(request_json("GET", "/health"), sort_keys=True), file=sys.stderr)

session_started_at = datetime.now(timezone.utc)
start = request_json("POST", f"/api/v1/devices/{device_id}/monitor/start", {})
initial_trace_count = int(start.get("initial_trace_count", start.get("trace_count", 0)) or 0)
initial_log_count = int(start.get("initial_log_count", start.get("log_count", 0)) or 0)
session_trace_count = int(start.get("trace_count", initial_trace_count) or 0)
session_log_count = int(start.get("log_count", initial_log_count) or 0)
if reset_on_attach:
    reset_snapshot = request_json("POST", f"/api/v1/devices/{device_id}/reset", {})
    print(json.dumps(reset_snapshot, sort_keys=True), file=sys.stderr)

probe_wait_sec = float(initial_stdout_timeout_sec if after_flash else duration_sec)
probe_wait_sec = min(probe_wait_sec, float(duration_sec))
fallback_reset_triggered = False
fallback_reset_at: datetime | None = None
if after_flash:
    time.sleep(probe_wait_sec)
    probe_snapshot = request_json(
        "GET",
        f"/api/v1/devices/{device_id}/trace?logs_limit=500&trace_limit=2000",
    )
    probe_trace = probe_snapshot.get("trace", [])
    if not isinstance(probe_trace, list):
        probe_trace = []
    probe_logs = probe_snapshot.get("logs", [])
    if not isinstance(probe_logs, list):
        probe_logs = []
    if not has_device_activity_since(
        probe_snapshot,
        session_trace_count,
        session_log_count,
        session_started_at,
    ):
        request_json("POST", f"/api/v1/devices/{device_id}/monitor/stop", {})
        reset_snapshot = None
        for _ in range(3):
            try:
                reset_snapshot = request_json("POST", f"/api/v1/devices/{device_id}/reset", {})
                break
            except SystemExit as exc:
                if "502" not in str(exc):
                    raise
                time.sleep(2)
        if reset_snapshot is None:
            raise SystemExit(
                f"POST /api/v1/devices/{device_id}/reset failed after fallback retries"
            )
        print(json.dumps(reset_snapshot, sort_keys=True), file=sys.stderr)
        fallback_reset_triggered = True
        fallback_reset_at = datetime.now(timezone.utc)
        session_started_at = datetime.now(timezone.utc)
        start = request_json("POST", f"/api/v1/devices/{device_id}/monitor/start", {})
        initial_trace_count = int(start.get("initial_trace_count", start.get("trace_count", 0)) or 0)
        initial_log_count = int(start.get("initial_log_count", start.get("log_count", 0)) or 0)
        session_trace_count = int(start.get("trace_count", initial_trace_count) or 0)
        session_log_count = int(start.get("log_count", initial_log_count) or 0)
    remaining_sec = max(0.0, float(duration_sec) - probe_wait_sec)
    if remaining_sec > 0:
        time.sleep(remaining_sec)
else:
    time.sleep(duration_sec)

trace_snapshot = request_json(
    "GET",
    f"/api/v1/devices/{device_id}/trace?logs_limit=500&trace_limit=2000",
)
request_json("POST", f"/api/v1/devices/{device_id}/monitor/stop", {})

entries: list[dict[str, object]] = [
    {
        "ts": fmt_ts(session_started_at),
        "src": "meta",
        "event": "monitor_session_start",
        "after_flash": after_flash,
        "reset_on_attach": reset_on_attach,
        "initial_trace_count": initial_trace_count,
        "initial_log_count": initial_log_count,
        "initial_stdout_timeout_sec": initial_stdout_timeout_sec,
    }
]
if fallback_reset_triggered:
    entries.append(
        {
            "ts": fmt_ts(fallback_reset_at or session_started_at),
            "src": "meta",
            "event": "post_flash_fallback_reset",
            "reason": "no_trace_or_log_activity_during_quiet_window",
            "quiet_window_sec": probe_wait_sec,
        }
    )

trace_entries = entries_since(
    trace_snapshot,
    "trace",
    "trace_count",
    session_trace_count,
    session_started_at,
)
log_entries = entries_since(
    trace_snapshot,
    "logs",
    "log_count",
    session_log_count,
    session_started_at,
)

for entry in trace_entries:
    if not isinstance(entry, dict):
        continue
    text = to_text(entry)
    if not text:
        continue
    entries.append(
        {
            "ts": entry.get("timestamp") or entry.get("ts") or now(),
            "src": "trace",
            "text": text,
            "direction": entry.get("direction"),
            "kind": entry.get("kind"),
            "frameType": entry.get("frameType"),
            "requestId": entry.get("requestId"),
            "target": entry.get("target"),
            "summary": entry.get("summary"),
            "payload": entry.get("payload"),
        }
    )

for entry in log_entries:
    if not isinstance(entry, dict):
        continue
    message = entry.get("message")
    if not isinstance(message, str) or not message.strip():
        continue
    entries.append(
        {
            "ts": entry.get("timestamp") or entry.get("ts") or now(),
            "src": "log",
            "text": message,
            "level": entry.get("level"),
            "target": entry.get("target"),
        }
    )

def sort_key(item: dict[str, object]) -> tuple[str, int]:
    ts = item.get("ts")
    if isinstance(ts, str):
        return (ts, 0)
    return (now(), 1)

entries.sort(key=sort_key)
output_path.parent.mkdir(parents=True, exist_ok=True)
with output_path.open("w", encoding="utf-8") as handle:
    for entry in entries:
        handle.write(json.dumps(entry, ensure_ascii=True) + "\n")

print(str(output_path))
PY
