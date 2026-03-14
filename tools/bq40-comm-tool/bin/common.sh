#!/usr/bin/env bash

bq40_tool_acquire_flash_monitor_lock() {
  local tool_root="$1"

  if [[ -n "${BQ40_TOOL_LOCK_HELD:-}" ]]; then
    return 0
  fi

  local lock_dir="$tool_root/.state"
  local lock_path="$lock_dir/flash-monitor.lock.d"
  local owner_file="$lock_path/owner"

  mkdir -p "$lock_dir"

  if ! mkdir "$lock_path" 2>/dev/null; then
    echo "bq40-comm-tool flash/monitor is busy; wait for the current session to finish before starting another one" >&2
    exit 71
  fi

  printf '%s\n' "$$" > "$owner_file"
  export BQ40_TOOL_LOCK_HELD="1"
  export BQ40_TOOL_LOCK_PATH="$lock_path"

  trap '
    if [[ "${BQ40_TOOL_LOCK_HELD:-}" == "1" && -d "${BQ40_TOOL_LOCK_PATH:-}" ]]; then
      rm -f "${BQ40_TOOL_LOCK_PATH}/owner"
      rmdir "${BQ40_TOOL_LOCK_PATH}" 2>/dev/null || true
    fi
  ' EXIT
}
