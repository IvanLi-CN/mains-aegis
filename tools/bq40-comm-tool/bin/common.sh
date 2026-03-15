#!/usr/bin/env bash

bq40_tool_lock_owner_age_secs() {
  python3 - "$1" <<'PY'
import os, sys, time
path = sys.argv[1]
try:
    print(int(max(0, time.time() - os.stat(path).st_mtime)))
except FileNotFoundError:
    print(-1)
PY
}

bq40_tool_acquire_flash_monitor_lock() {
  local tool_root="$1"

  if [[ -n "${BQ40_TOOL_LOCK_HELD:-}" ]]; then
    return 0
  fi

  local lock_dir="$tool_root/.state"
  local lock_path="$lock_dir/flash-monitor.lock.d"
  local owner_file="$lock_path/owner"
  local owner_pid=""
  local owner_start=""
  local owner_cmd=""
  local current_start=""
  local current_cmd=""

  mkdir -p "$lock_dir"

  if ! mkdir "$lock_path" 2>/dev/null; then
    if [[ ! -f "$owner_file" ]]; then
      local wait_loops=0
      while [[ ! -f "$owner_file" && $wait_loops -lt 10 ]]; do
        sleep 0.1
        wait_loops=$((wait_loops + 1))
      done
    fi
    if [[ -f "$owner_file" ]]; then
      owner_pid="$(sed -n 's/^pid=//p' "$owner_file" | head -n1 | tr -d '[:space:]')"
      owner_start="$(sed -n 's/^start=//p' "$owner_file" | head -n1)"
      owner_cmd="$(sed -n 's/^cmd=//p' "$owner_file" | head -n1)"
    fi
    if [[ "$owner_pid" =~ ^[0-9]+$ ]]; then
      current_start="$(ps -p "$owner_pid" -o lstart= 2>/dev/null | sed 's/^ *//')"
      current_cmd="$(ps -p "$owner_pid" -o command= 2>/dev/null | sed 's/^ *//')"
    fi
    if [[ "$owner_pid" =~ ^[0-9]+$ ]] \
      && [[ -n "$current_start" ]] \
      && [[ "$current_start" == "$owner_start" ]] \
      && [[ "$current_cmd" == "$owner_cmd" ]]; then
      echo "bq40-comm-tool flash/monitor is busy; wait for the current session to finish before starting another one" >&2
      exit 71
    else
      local lock_age
      lock_age="$(bq40_tool_lock_owner_age_secs "$lock_path")"
      if [[ ! -f "$owner_file" && "$lock_age" -ge 0 && "$lock_age" -lt 5 ]]; then
        echo "bq40-comm-tool flash/monitor is busy; lock acquisition is still in progress" >&2
        exit 71
      fi
      rm -f "$owner_file"
      rmdir "$lock_path" 2>/dev/null || true
      if mkdir "$lock_path" 2>/dev/null; then
        :
      else
        echo "bq40-comm-tool flash/monitor is busy; stale lock recovery failed" >&2
        exit 71
      fi
    fi
  fi

  printf 'pid=%s\nstart=%s\ncmd=%s\n' \
    "$$" \
    "$(ps -p "$$" -o lstart= 2>/dev/null | sed 's/^ *//')" \
    "$(ps -p "$$" -o command= 2>/dev/null | sed 's/^ *//')" \
    > "$owner_file"
  export BQ40_TOOL_LOCK_HELD="1"
  export BQ40_TOOL_LOCK_PATH="$lock_path"

  trap '
    if [[ "${BQ40_TOOL_LOCK_HELD:-}" == "1" && -d "${BQ40_TOOL_LOCK_PATH:-}" ]]; then
      rm -f "${BQ40_TOOL_LOCK_PATH}/owner"
      rmdir "${BQ40_TOOL_LOCK_PATH}" 2>/dev/null || true
    fi
  ' EXIT
}
