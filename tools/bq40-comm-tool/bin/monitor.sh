#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

duration_sec=120
output_file=""
after_flash="false"
reset_on_attach="false"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--duration-sec N] [--output PATH] [--after-flash true|false] [--reset-on-attach true|false]
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

monitor_path=$(python3 - "$TOOL_ROOT" "$duration_sec" "$after_flash" "$reset_on_attach" <<'PY'
import json
import os
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from collections import deque
from pathlib import Path
from threading import Lock, Thread
from typing import Deque, Dict, Optional, Tuple

INITIAL_STDOUT_TIMEOUT_SEC = 6.0
RECENT_EXISTING_STDOUT_GRACE_SEC = 10.0

root = Path(sys.argv[1])
duration = int(sys.argv[2])
after_flash = sys.argv[3] == "true"
reset_on_attach = sys.argv[4] == "true"
monitor_dir = root / ".mcu-agentd" / "monitor" / "esp"
monitor_dir.mkdir(parents=True, exist_ok=True)
combined_fd, combined_tmp = tempfile.mkstemp(
    prefix=f"{time.strftime('%Y%m%d_%H%M%S')}_",
    suffix="_combined.mon.ndjson",
    dir=monitor_dir,
)
os.close(combined_fd)
combined_path = Path(combined_tmp)


def snapshot() -> Dict[Path, Tuple[float, int]]:
    state: Dict[Path, Tuple[float, int]] = {}
    for p in monitor_dir.glob("*.mon.ndjson"):
        st = p.stat()
        state[p.resolve()] = (st.st_mtime, st.st_size)
    return state


def capture(stream, tail: Deque[str], parse_payload: bool, path_ref: Dict[str, Optional[Path]]) -> None:
    for raw in iter(stream.readline, ""):
        line = raw.rstrip("\n")
        if line:
            tail.append(line)
        if not parse_payload:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        payload = entry.get("payload")
        if not isinstance(payload, dict):
            continue
        payload_path = payload.get("path")
        if isinstance(payload_path, str) and payload_path.endswith(".mon.ndjson"):
            candidate = Path(payload_path)
            if not candidate.is_absolute():
                candidate = root / candidate
            if path_ref["path"] is None:
                path_ref["path"] = candidate.resolve()
    stream.close()


def resolve_monitor_path(
    before: Dict[Path, Tuple[float, int]],
    after: Dict[Path, Tuple[float, int]],
    started_at: float,
    hinted_path: Optional[Path],
) -> Optional[Path]:
    changed = []
    for path, (mtime, size) in after.items():
        prev = before.get(path)
        if prev is None or mtime > prev[0] or size > prev[1]:
            changed.append((mtime, path))
    changed.sort()
    changed_paths = {path for _, path in changed}

    if (
        hinted_path is not None
        and hinted_path.exists()
        and (
            hinted_path in changed_paths
            or (hinted_path in after and after[hinted_path][0] >= started_at)
        )
    ):
        return hinted_path
    if changed:
        return changed[-1][1]
    return None


def append_segment(
    src: Path,
    before: Dict[Path, Tuple[float, int]],
    appended_offsets: Dict[Path, int],
) -> None:
    src = src.resolve()
    if not src.exists():
        return
    baseline_offset = before.get(src, (0.0, 0))[1]
    prev_offset = appended_offsets.get(src, baseline_offset)
    size = src.stat().st_size
    if size < prev_offset:
        prev_offset = baseline_offset if size >= baseline_offset else 0
    with src.open("rb") as infile, combined_path.open("ab") as outfile:
        infile.seek(prev_offset)
        chunk = infile.read()
        if not chunk:
            appended_offsets[src] = infile.tell()
            return
        outfile.write(chunk)
        appended_offsets[src] = infile.tell()


def parse_entry_ts(entry: dict) -> Optional[float]:
    ts = entry.get("ts")
    if not isinstance(ts, str):
        return None
    normalized = ts[:-1] + "+00:00" if ts.endswith("Z") else ts
    try:
        return datetime.fromisoformat(normalized).astimezone(timezone.utc).timestamp()
    except ValueError:
        return None


def monitor_file_has_stdout(
    src: Optional[Path],
    before: Dict[Path, Tuple[float, int]],
    started_at: float,
    allow_recent_existing: bool,
) -> bool:
    if src is None or not src.exists():
        return False
    src = src.resolve()
    baseline_offset = before.get(src, (0.0, 0))[1]
    latest_existing_stdout_ts: Optional[float] = None
    try:
        with src.open("r", encoding="utf-8") as infile:
            while True:
                line_start = infile.tell()
                line = infile.readline()
                if not line:
                    break
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if entry.get("src") != "stdout":
                    continue
                if line_start >= baseline_offset:
                    return True
                if allow_recent_existing:
                    parsed_ts = parse_entry_ts(entry)
                    if parsed_ts is not None:
                        latest_existing_stdout_ts = parsed_ts
    except OSError:
        return False

    return (
        allow_recent_existing
        and latest_existing_stdout_ts is not None
        and latest_existing_stdout_ts >= started_at - RECENT_EXISTING_STDOUT_GRACE_SEC
    )


def stop_process(proc: subprocess.Popen[str]) -> None:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


def inspect_monitor_path(
    before: Dict[Path, Tuple[float, int]],
    started_at: float,
    hinted_path: Optional[Path],
    allow_recent_existing: bool,
) -> Tuple[Dict[Path, Tuple[float, int]], Optional[Path], bool]:
    after = snapshot()
    chosen = resolve_monitor_path(before, after, started_at, hinted_path)
    allow_existing_for_chosen = (
        allow_recent_existing
        and hinted_path is not None
        and chosen is not None
        and chosen == hinted_path.resolve()
    )
    return after, chosen, monitor_file_has_stdout(
        chosen,
        before,
        started_at,
        allow_existing_for_chosen,
    )


def wait_for_target_stdout(
    proc: subprocess.Popen[str],
    before: Dict[Path, Tuple[float, int]],
    started_at: float,
    path_ref: Dict[str, Optional[Path]],
    timeout_sec: float,
    allow_recent_existing: bool,
) -> Tuple[Dict[Path, Tuple[float, int]], Optional[Path], bool]:
    probe_deadline = time.time() + timeout_sec
    after = before
    chosen: Optional[Path] = None
    has_stdout = False
    while time.time() < probe_deadline:
        after, chosen, has_stdout = inspect_monitor_path(before, started_at, path_ref["path"], allow_recent_existing)
        if has_stdout or proc.poll() is not None:
            return after, chosen, has_stdout
        time.sleep(0.2)
    return inspect_monitor_path(before, started_at, path_ref["path"], allow_recent_existing)


deadline: Optional[float] = None
appended_offsets: Dict[Path, int] = {}
restarts = 0
first_attach = True
last_detail = ""
use_reset_attach = reset_on_attach
reset_fallback_used = False

while True:
    if deadline is not None and time.time() >= deadline:
        break
    remaining = duration if deadline is None else max(1.0, deadline - time.time())
    before = snapshot()
    started_at = time.time()
    stdout_tail: Deque[str] = deque(maxlen=200)
    stderr_tail: Deque[str] = deque(maxlen=200)
    path_ref: Dict[str, Optional[Path]] = {"path": None}

    cmd = ["mcu-agentd", "--non-interactive", "monitor", "esp"]
    # Prefer reusing the daemon-started monitor after flashing. If that attach fails to show any
    # target stdout within a short window, fall back once to a controlled reset attach so the
    # freshly flashed firmware actually boots and starts logging.
    if use_reset_attach:
        cmd.append("--reset")

    proc = subprocess.Popen(
        cmd,
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        bufsize=1,
    )
    if proc.stdout is None or proc.stderr is None:
        raise SystemExit("mcu-agentd monitor failed: missing stdout/stderr pipe")

    stdout_thread = Thread(
        target=capture,
        args=(proc.stdout, stdout_tail, True, path_ref),
        daemon=True,
    )
    stderr_thread = Thread(
        target=capture,
        args=(proc.stderr, stderr_tail, False, path_ref),
        daemon=True,
    )
    stdout_thread.start()
    stderr_thread.start()

    timed_out = False
    after = before
    chosen: Optional[Path] = None
    chosen_has_stdout = False

    if after_flash and not use_reset_attach and not reset_fallback_used:
        probe_timeout = min(INITIAL_STDOUT_TIMEOUT_SEC, remaining)
        after, chosen, chosen_has_stdout = wait_for_target_stdout(
            proc,
            before,
            started_at,
            path_ref,
            probe_timeout,
            True,
        )
        if not chosen_has_stdout:
            if proc.poll() is None:
                stop_process(proc)
            stdout_thread.join(timeout=2)
            stderr_thread.join(timeout=2)

            stdout_data = "\n".join(stdout_tail)
            stderr_data = "\n".join(stderr_tail)
            if chosen is not None:
                append_segment(chosen, before, appended_offsets)
            detail_lines = (stderr_data or stdout_data).strip().splitlines()[-8:]
            last_detail = "\n".join(detail_lines) if detail_lines else f"mcu-agentd exited with {proc.returncode}"
            reset_fallback_used = True
            use_reset_attach = True
            time.sleep(0.5)
            continue

    if deadline is None:
        deadline = time.time() + duration
    remaining = max(0.1, deadline - time.time())
    try:
        proc.wait(timeout=remaining)
    except subprocess.TimeoutExpired:
        timed_out = True
        stop_process(proc)

    stdout_thread.join(timeout=2)
    stderr_thread.join(timeout=2)

    stdout_data = "\n".join(stdout_tail)
    stderr_data = "\n".join(stderr_tail)
    after, chosen, chosen_has_stdout = inspect_monitor_path(before, started_at, path_ref["path"], False)
    if chosen is not None:
        append_segment(chosen, before, appended_offsets)

    detail_lines = (stderr_data or stdout_data).strip().splitlines()[-8:]
    last_detail = "\n".join(detail_lines) if detail_lines else f"mcu-agentd exited with {proc.returncode}"

    if timed_out:
        break

    use_reset_attach = False
    if chosen_has_stdout:
        reset_fallback_used = False

    if proc.returncode == 0:
        if time.time() >= deadline:
            break
        first_attach = False
        time.sleep(0.2)
        continue

    if first_attach:
        restarts += 1
        if restarts > 8:
            break
        first_attach = False
        time.sleep(0.5)
        continue

    if combined_path.exists() and combined_path.stat().st_size > 0:
        restarts += 1
        if restarts > 8:
            break
        first_attach = False
        time.sleep(0.5)
        continue

    raise SystemExit(f"mcu-agentd monitor failed (rc={proc.returncode})\n{last_detail}")

if not combined_path.exists() or combined_path.stat().st_size == 0:
    detail = last_detail or "no monitor output captured"
    raise SystemExit(f"monitor output not found for this run\n{detail}")

print(combined_path.resolve())
PY
)

if [[ -n "$output_file" ]]; then
  mkdir -p "$(dirname "$output_file")"
  cp "$monitor_path" "$output_file"
  monitor_path="$(cd "$(dirname "$output_file")" && pwd)/$(basename "$output_file")"
fi

echo "$monitor_path"
