#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
devd_bin="${repo_root}/tools/mains-aegis-host/target/debug/mains-aegis-devd"
if [[ -n "${HOST_POWER_MACOS_API_PORT:-}" ]]; then
  api_port="${HOST_POWER_MACOS_API_PORT}"
else
  api_port="$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
fi
ipc_path="${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.sock"

if [[ ! -x "${devd_bin}" ]]; then
  echo "Missing devd binary: ${devd_bin}" >&2
  exit 1
fi

initial_low_power="$(pmset -g | awk '/lowpowermode/ {print $2; exit}')"
profile_available=1
if [[ "${initial_low_power}" != "0" && "${initial_low_power}" != "1" ]]; then
  profile_available=0
  initial_low_power="0"
  echo "macOS runner VM does not expose pmset lowpowermode; validating diagnostic error and shutdown command." >&2
  pmset -g >&2
fi

devd_pid=""
cleanup() {
  sudo killall shutdown >/dev/null 2>&1 || true
  if [[ "${profile_available}" == "1" ]]; then
    sudo pmset -a lowpowermode "${initial_low_power}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${devd_pid}" ]] && kill -0 "${devd_pid}" >/dev/null 2>&1; then
    sudo kill "${devd_pid}" >/dev/null 2>&1 || true
    wait "${devd_pid}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

rm -f "${ipc_path}"
sudo env MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=1 \
  "${devd_bin}" bridge-http --ipc "${ipc_path}" --bind "127.0.0.1:${api_port}" \
  > "${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.log" 2>&1 &
devd_pid=$!

for _ in {1..60}; do
  if curl -fsS "http://127.0.0.1:${api_port}/api/v1/host/power" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! curl -fsS "http://127.0.0.1:${api_port}/api/v1/host/power" >/dev/null; then
  cat "${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.log" >&2 || true
  exit 1
fi

if [[ "${profile_available}" == "1" ]]; then
  curl -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/profile" \
    -H 'content-type: application/json' \
    -d '{"profile":"power_saver","dry_run":false}' | python3 -c 'import json,sys; assert json.load(sys.stdin)["ok"] is True'

  after_low_power="$(pmset -g | awk '/lowpowermode/ {print $2; exit}')"
  if [[ "${after_low_power}" != "1" ]]; then
    echo "Expected macOS runner VM lowpowermode=1, got ${after_low_power}" >&2
    cat "${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.log" >&2 || true
    exit 1
  fi

  curl -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/profile" \
    -H 'content-type: application/json' \
    -d '{"profile":"balanced","dry_run":false}' | python3 -c 'import json,sys; assert json.load(sys.stdin)["ok"] is True'

  restored_low_power="$(pmset -g | awk '/lowpowermode/ {print $2; exit}')"
  if [[ "${restored_low_power}" != "0" ]]; then
    echo "Expected macOS runner VM lowpowermode=0 after restore, got ${restored_low_power}" >&2
    cat "${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.log" >&2 || true
    exit 1
  fi
else
  curl -sS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/profile" \
    -H 'content-type: application/json' \
    -d '{"profile":"power_saver","dry_run":true}' \
    | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["error"]["code"] == "host_power_profile_query_failed"'
fi

curl -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/shutdown" \
  -H 'content-type: application/json' \
  -d '{"delay_sec":600,"dry_run":false,"confirm":"shutdown"}' \
  | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data["ok"] is True and data["dispatch"] == "command_accepted"'

if ! pgrep -x shutdown >/dev/null 2>&1; then
  echo "Expected macOS shutdown scheduler process after devd shutdown command." >&2
  cat "${RUNNER_TEMP:-/tmp}/mains-aegis-devd-macos.log" >&2 || true
  exit 1
fi

sudo killall shutdown
