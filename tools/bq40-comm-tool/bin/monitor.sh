#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOOL_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$TOOL_ROOT" && git rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "$REPO_ROOT" ]]; then
  REPO_ROOT=$(cd "$TOOL_ROOT/../.." && pwd)
fi

duration_sec=120
output_file=""

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--duration-sec N] [--output PATH]
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --duration-sec)
      duration_sec="${2:-}"
      shift 2
      ;;
    --output)
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

monitor_path=$(python3 - "$REPO_ROOT" "$duration_sec" <<'PY'
import json
import subprocess
import sys
from pathlib import Path
from typing import Dict, Optional, Tuple

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

path_from_stdout: Optional[Path] = None
try:
    result = subprocess.run(
        ["mcu-agentd", "--non-interactive", "monitor", "esp", "--reset"],
        cwd=root,
        timeout=duration,
        text=True,
        capture_output=True,
    )
    stdout_data = result.stdout or ""
    stderr_data = result.stderr or ""
except subprocess.TimeoutExpired as exc:
    stdout_data = (exc.stdout.decode("utf-8", errors="replace") if isinstance(exc.stdout, bytes) else (exc.stdout or ""))
    stderr_data = (exc.stderr.decode("utf-8", errors="replace") if isinstance(exc.stderr, bytes) else (exc.stderr or ""))

for line in stdout_data.splitlines():
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
        path_from_stdout = candidate.resolve()
        break

if "result" in locals() and result.returncode != 0:
    tail = (stderr_data or stdout_data).strip().splitlines()[-8:]
    detail = "\n".join(tail) if tail else f"mcu-agentd exited with {result.returncode}"
    raise SystemExit(f"mcu-agentd monitor failed (rc={result.returncode})\n{detail}")

after = snapshot()
changed = []
for path, (mtime, size) in after.items():
    prev = before.get(path)
    if prev is None or mtime > prev[0] or size > prev[1]:
        changed.append((mtime, path))
changed.sort()

chosen: Optional[Path] = None
if path_from_stdout is not None and path_from_stdout.exists():
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
