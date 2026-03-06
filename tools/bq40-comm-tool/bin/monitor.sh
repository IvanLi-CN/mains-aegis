#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

duration_sec=120
output_file=""

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--duration-sec N] [--output PATH]
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

monitor_path=$(python3 - "$TOOL_ROOT" "$duration_sec" <<'PY'
import json
import subprocess
import sys
import time
from collections import deque
from pathlib import Path
from threading import Lock, Thread
from typing import Deque, Dict, Optional, Set, Tuple

root = Path(sys.argv[1])
duration = int(sys.argv[2])
monitor_dir = root / ".mcu-agentd" / "monitor" / "esp"
monitor_dir.mkdir(parents=True, exist_ok=True)
combined_path = monitor_dir / f"{time.strftime('%Y%m%d_%H%M%S')}_combined.mon.ndjson"


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


def append_segment(src: Path, appended: Set[Path]) -> None:
    src = src.resolve()
    if src in appended or not src.exists():
        return
    with src.open("rb") as infile, combined_path.open("ab") as outfile:
        outfile.write(infile.read())
    appended.add(src)


def stop_process(proc: subprocess.Popen[str]) -> None:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


deadline = time.time() + duration
appended_paths: Set[Path] = set()
restarts = 0
first_attach = True
last_detail = ""

while time.time() < deadline:
    remaining = max(1.0, deadline - time.time())
    before = snapshot()
    started_at = time.time()
    stdout_tail: Deque[str] = deque(maxlen=200)
    stderr_tail: Deque[str] = deque(maxlen=200)
    path_ref: Dict[str, Optional[Path]] = {"path": None}

    cmd = ["mcu-agentd", "--non-interactive", "monitor", "esp"]
    if first_attach:
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
    try:
        proc.wait(timeout=remaining)
    except subprocess.TimeoutExpired:
        timed_out = True
        stop_process(proc)

    stdout_thread.join(timeout=2)
    stderr_thread.join(timeout=2)

    stdout_data = "\n".join(stdout_tail)
    stderr_data = "\n".join(stderr_tail)
    after = snapshot()
    chosen = resolve_monitor_path(before, after, started_at, path_ref["path"])
    if chosen is not None:
        append_segment(chosen, appended_paths)

    detail_lines = (stderr_data or stdout_data).strip().splitlines()[-8:]
    last_detail = "\n".join(detail_lines) if detail_lines else f"mcu-agentd exited with {proc.returncode}"

    if timed_out:
        break

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
