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
from typing import Deque, Dict, Optional, Tuple

root = Path(sys.argv[1])
duration = int(sys.argv[2])
monitor_dir = root / ".mcu-agentd" / "monitor" / "esp"
monitor_dir.mkdir(parents=True, exist_ok=True)

def snapshot() -> Dict[Path, Tuple[float, int]]:
    state: Dict[Path, Tuple[float, int]] = {}
    for p in monitor_dir.glob("*.mon.ndjson"):
        st = p.stat()
        state[p.resolve()] = (st.st_mtime, st.st_size)
    return state

before = snapshot()
started_at = time.time()

stdout_tail: Deque[str] = deque(maxlen=200)
stderr_tail: Deque[str] = deque(maxlen=200)
path_lock = Lock()
path_from_stdout: Optional[Path] = None


def capture(stream, tail: Deque[str], parse_payload: bool) -> None:
    global path_from_stdout
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
            with path_lock:
                if path_from_stdout is None:
                    path_from_stdout = candidate.resolve()
    stream.close()


proc = subprocess.Popen(
    ["mcu-agentd", "--non-interactive", "monitor", "esp", "--reset"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    bufsize=1,
)
if proc.stdout is None or proc.stderr is None:
    raise SystemExit("mcu-agentd monitor failed: missing stdout/stderr pipe")

stdout_thread = Thread(target=capture, args=(proc.stdout, stdout_tail, True), daemon=True)
stderr_thread = Thread(target=capture, args=(proc.stderr, stderr_tail, False), daemon=True)
stdout_thread.start()
stderr_thread.start()

timed_out = False
try:
    proc.wait(timeout=duration)
except subprocess.TimeoutExpired:
    timed_out = True
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)

stdout_thread.join(timeout=2)
stderr_thread.join(timeout=2)

stdout_data = "\n".join(stdout_tail)
stderr_data = "\n".join(stderr_tail)

if not timed_out and proc.returncode != 0:
    tail = (stderr_data or stdout_data).strip().splitlines()[-8:]
    detail = "\n".join(tail) if tail else f"mcu-agentd exited with {proc.returncode}"
    raise SystemExit(f"mcu-agentd monitor failed (rc={proc.returncode})\n{detail}")

after = snapshot()
changed = []
for path, (mtime, size) in after.items():
    prev = before.get(path)
    if prev is None or mtime > prev[0] or size > prev[1]:
        changed.append((mtime, path))
changed.sort()
changed_paths = {path for _, path in changed}

chosen: Optional[Path] = None
if (
    path_from_stdout is not None
    and path_from_stdout.exists()
    and (
        path_from_stdout in changed_paths
        or (
            path_from_stdout in after
            and after[path_from_stdout][0] >= started_at
        )
    )
):
    chosen = path_from_stdout
elif changed:
    chosen = changed[-1][1]

if chosen is None:
    tail = (stderr_data or stdout_data).strip().splitlines()[-5:]
    detail = "\n".join(tail) if tail else "no monitor output captured"
    raise SystemExit(f"monitor output not found for this run\n{detail}")

print(chosen)
PY
)

if [[ -n "$output_file" ]]; then
  mkdir -p "$(dirname "$output_file")"
  cp "$monitor_path" "$output_file"
  monitor_path="$(cd "$(dirname "$output_file")" && pwd)/$(basename "$output_file")"
fi

echo "$monitor_path"
