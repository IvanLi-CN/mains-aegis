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
from threading import Thread
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
captured_monitor_bytes = 0
captured_monitor_new_bytes = 0
saw_recent_existing_stdout = False


def snapshot() -> Dict[Path, Tuple[float, int]]:
    state: Dict[Path, Tuple[float, int]] = {}
    for p in monitor_dir.glob("*.mon.ndjson"):
        resolved = p.resolve()
        if resolved == combined_path.resolve():
            continue
        st = p.stat()
        state[resolved] = (st.st_mtime, st.st_size)
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
            # Prefer the most recently hinted file path (flash may start monitor before we attach).
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

    if hinted_path is not None:
        hinted_path = hinted_path.resolve()
        # A hint coming from the current attach (or meta) is the strongest signal, even if the file
        # existed before monitor.sh started and no new bytes arrived yet.
        if hinted_path.exists():
            return hinted_path
    if changed:
        return changed[-1][1]
    return None


def append_meta_entry(event: str, **fields: object) -> None:
    entry = {
        "ts": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "mcu_id": "esp",
        "src": "meta",
        "event": event,
    }
    entry.update(fields)
    with combined_path.open("a", encoding="utf-8") as outfile:
        outfile.write(json.dumps(entry, ensure_ascii=True) + "\n")


def append_segment(
    src: Path,
    before: Dict[Path, Tuple[float, int]],
    appended_offsets: Dict[Path, int],
    start_offset_override: Optional[int] = None,
) -> None:
    global captured_monitor_bytes, captured_monitor_new_bytes
    src = src.resolve()
    if src == combined_path.resolve() or not src.exists():
        return
    baseline_offset = before.get(src, (0.0, 0))[1]
    had_prev_offset = src in appended_offsets
    prev_offset = appended_offsets.get(src, baseline_offset)
    # `start_offset_override` is used to capture a "recent existing stdout" window on the very
    # first attach. Never rewind offsets on subsequent attaches, or the same bytes will be
    # duplicated in the combined log and inflate parser counters.
    if (
        start_offset_override is not None
        and not had_prev_offset
        and start_offset_override < prev_offset
    ):
        prev_offset = max(0, start_offset_override)
    size = src.stat().st_size
    if size < prev_offset:
        prev_offset = baseline_offset if size >= baseline_offset else 0
    new_start = max(baseline_offset, prev_offset)
    with src.open("rb") as infile, combined_path.open("ab") as outfile:
        infile.seek(prev_offset)
        chunk = infile.read()
        if not chunk:
            appended_offsets[src] = infile.tell()
            return
        outfile.write(chunk)
        captured_monitor_bytes += len(chunk)
        if size > new_start:
            captured_monitor_new_bytes += size - new_start
        appended_offsets[src] = infile.tell()


def parse_entry_ts(entry: dict) -> Optional[float]:
    ts = entry.get("ts") or entry.get("timestamp")
    if not isinstance(ts, str):
        return None
    normalized = ts[:-1] + "+00:00" if ts.endswith("Z") else ts
    try:
        return datetime.fromisoformat(normalized).astimezone(timezone.utc).timestamp()
    except ValueError:
        return None


def monitor_file_stdout_window(
    src: Optional[Path],
    before: Dict[Path, Tuple[float, int]],
    started_at: float,
    allow_recent_existing: bool,
) -> Tuple[bool, Optional[int]]:
    if src is None or not src.exists():
        return False, None
    src = src.resolve()
    baseline_offset = before.get(src, (0.0, 0))[1]
    latest_existing_stdout_ts: Optional[float] = None
    recent_window_offset: Optional[int] = None
    recent_window_start = started_at - RECENT_EXISTING_STDOUT_GRACE_SEC
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
                parsed_ts = parse_entry_ts(entry)
                if (
                    allow_recent_existing
                    and line_start < baseline_offset
                    and parsed_ts is not None
                    and parsed_ts >= recent_window_start
                    and recent_window_offset is None
                ):
                    recent_window_offset = line_start
                if entry.get("src") != "stdout":
                    continue
                if line_start >= baseline_offset:
                    return True, None
                if allow_recent_existing and parsed_ts is not None:
                    latest_existing_stdout_ts = parsed_ts
    except OSError:
        return False, None

    if (
        allow_recent_existing
        and latest_existing_stdout_ts is not None
        and latest_existing_stdout_ts >= recent_window_start
    ):
        # Treat recent pre-attach stdout as attach evidence in after-flash mode so we don't
        # immediately force a reset and lose the post-flash trace. This does NOT imply any
        # new bytes were written after monitor.sh started.
        return False, recent_window_offset
    return False, None


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
) -> Tuple[Dict[Path, Tuple[float, int]], Optional[Path], bool, Optional[int]]:
    after = snapshot()
    chosen = resolve_monitor_path(before, after, started_at, hinted_path)
    allow_existing_for_chosen = (
        allow_recent_existing
        and hinted_path is not None
        and chosen is not None
        and chosen == hinted_path.resolve()
    )
    has_stdout, recent_window_offset = monitor_file_stdout_window(
        chosen,
        before,
        started_at,
        allow_existing_for_chosen,
    )
    return after, chosen, has_stdout, recent_window_offset


def wait_for_target_stdout(
    proc: subprocess.Popen[str],
    before: Dict[Path, Tuple[float, int]],
    started_at: float,
    path_ref: Dict[str, Optional[Path]],
    timeout_sec: float,
    allow_recent_existing: bool,
) -> Tuple[Dict[Path, Tuple[float, int]], Optional[Path], bool, Optional[int]]:
    probe_deadline = time.time() + timeout_sec
    after = before
    chosen: Optional[Path] = None
    has_stdout = False
    recent_window_offset: Optional[int] = None
    while time.time() < probe_deadline:
        after, chosen, has_stdout, recent_window_offset = inspect_monitor_path(
            before,
            started_at,
            path_ref["path"],
            allow_recent_existing,
        )
        if has_stdout or proc.poll() is not None:
            return after, chosen, has_stdout, recent_window_offset
        time.sleep(0.2)
    return inspect_monitor_path(before, started_at, path_ref["path"], allow_recent_existing)


deadline: Optional[float] = None
appended_offsets: Dict[Path, int] = {}
restarts = 0
first_attach = True
last_detail = ""
use_reset_attach = reset_on_attach
reset_fallback_used = False
completed_duration = False

while True:
    if deadline is not None and time.time() >= deadline:
        completed_duration = True
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

    append_meta_entry(
        "monitor_session_start",
        after_flash=after_flash,
        reset_on_attach=use_reset_attach,
        attempt=restarts + 1,
    )

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
    recent_window_offset: Optional[int] = None

    if after_flash and not use_reset_attach and not reset_fallback_used:
        probe_timeout = min(INITIAL_STDOUT_TIMEOUT_SEC, remaining)
        after, chosen, chosen_has_stdout, recent_window_offset = wait_for_target_stdout(
            proc,
            before,
            started_at,
            path_ref,
            probe_timeout,
            True,
        )
        if chosen is not None and recent_window_offset is not None:
            saw_recent_existing_stdout = True
            append_meta_entry(
                "recent_existing_stdout",
                path=str(chosen.resolve()),
                grace_sec=RECENT_EXISTING_STDOUT_GRACE_SEC,
            )
            append_segment(chosen, before, appended_offsets, recent_window_offset)
        if not chosen_has_stdout and recent_window_offset is None:
            if proc.poll() is None:
                stop_process(proc)
            stdout_thread.join(timeout=2)
            stderr_thread.join(timeout=2)

            stdout_data = "\n".join(stdout_tail)
            stderr_data = "\n".join(stderr_tail)
            if chosen is not None:
                append_segment(chosen, before, appended_offsets, recent_window_offset)
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
    after, chosen, chosen_has_stdout, _ = inspect_monitor_path(before, started_at, path_ref["path"], False)
    if chosen is not None:
        append_segment(chosen, before, appended_offsets)

    detail_lines = (stderr_data or stdout_data).strip().splitlines()[-8:]
    last_detail = "\n".join(detail_lines) if detail_lines else f"mcu-agentd exited with {proc.returncode}"

    if timed_out or (deadline is not None and time.time() >= deadline):
        completed_duration = True
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

    # combined_path always contains at least the per-attach meta marker, so use the
    # captured monitor payload bytes (stdout/meta from mcu-agentd) instead of file size
    # to decide whether a retry can make progress.
    if captured_monitor_bytes > 0:
        restarts += 1
        if restarts > 8:
            break
        first_attach = False
        time.sleep(0.5)
        continue

    raise SystemExit(f"mcu-agentd monitor failed (rc={proc.returncode})\n{last_detail}")

if captured_monitor_new_bytes == 0:
    detail = last_detail or "no new monitor output captured"
    extra = ""
    if saw_recent_existing_stdout:
        extra = "\n(recent pre-attach stdout was found, but no new bytes appeared after this run started)"
    raise SystemExit(f"monitor output not found for this run\n{detail}{extra}")

if not completed_duration:
    detail = last_detail or "monitor session ended before requested duration"
    raise SystemExit(f"monitor run ended before requested duration\n{detail}")

print(combined_path.resolve())
PY
)

if [[ -n "$output_file" ]]; then
  mkdir -p "$(dirname "$output_file")"
  output_abs="$(cd "$(dirname "$output_file")" && pwd)/$(basename "$output_file")"
  cp "$monitor_path" "$output_file"
  # monitor_path points at the per-run combined temp file. When the caller requested
  # an explicit output path, clean up the temp file (unless they happened to choose
  # that exact same path).
  if [[ "$monitor_path" != "$output_abs" ]]; then
    rm -f "$monitor_path"
  fi
  monitor_path="$output_abs"
fi

echo "$monitor_path"
