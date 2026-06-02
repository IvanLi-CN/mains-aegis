#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work_dir="${RUNNER_TEMP:-/tmp}/mains-aegis-host-power-linux-vm"
image_url="${HOST_POWER_UBUNTU_IMAGE_URL:-https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img}"
ssh_port="${HOST_POWER_LINUX_VM_SSH_PORT:-2222}"
api_port="${HOST_POWER_LINUX_VM_API_PORT:-30080}"
devd_bin="${repo_root}/tools/mains-aegis-host/target/debug/mains-aegis-devd"
bridge_token="host-power-ci-token"
auth_header=(-H "authorization: Bearer ${bridge_token}")

if [[ ! -x "${devd_bin}" ]]; then
  echo "Missing devd binary: ${devd_bin}" >&2
  exit 1
fi

rm -rf "${work_dir}"
mkdir -p "${work_dir}"
ssh_key="${work_dir}/id_ed25519"
ssh-keygen -q -t ed25519 -N "" -f "${ssh_key}"

curl -fsSL "${image_url}" -o "${work_dir}/ubuntu-cloud.img"
qemu-img create -f qcow2 -F qcow2 -b "${work_dir}/ubuntu-cloud.img" "${work_dir}/guest.qcow2" 12G

cat > "${work_dir}/user-data" <<EOF
#cloud-config
users:
  - name: ci
    groups: [adm, sudo]
    shell: /bin/bash
    sudo: ["ALL=(ALL) NOPASSWD:ALL"]
    ssh_authorized_keys:
      - $(cat "${ssh_key}.pub")
packages:
  - curl
  - jq
  - power-profiles-daemon
package_update: true
runcmd:
  - systemctl enable --now ssh
  - systemctl enable --now power-profiles-daemon || true
EOF
cat > "${work_dir}/meta-data" <<EOF
instance-id: mains-aegis-host-power-test
local-hostname: mains-aegis-host-power-test
EOF
cloud-localds "${work_dir}/seed.iso" "${work_dir}/user-data" "${work_dir}/meta-data"

accel_args=(-accel tcg)
cpu_args=(-cpu max)
if [[ -e /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]; then
  accel_args=(-accel kvm)
  cpu_args=(-cpu host)
fi

qemu-system-x86_64 \
  "${accel_args[@]}" \
  -machine q35 \
  "${cpu_args[@]}" \
  -smp 2 \
  -m 2048 \
  -display none \
  -serial "file:${work_dir}/serial.log" \
  -drive "file=${work_dir}/guest.qcow2,if=virtio,format=qcow2" \
  -drive "file=${work_dir}/seed.iso,if=virtio,format=raw,readonly=on" \
  -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${ssh_port}-:22,hostfwd=tcp:127.0.0.1:${api_port}-:30080" \
  -device virtio-net-pci,netdev=net0 \
  -pidfile "${work_dir}/qemu.pid" &
qemu_pid=$!

cleanup() {
  if kill -0 "${qemu_pid}" >/dev/null 2>&1; then
    kill "${qemu_pid}" >/dev/null 2>&1 || true
    wait "${qemu_pid}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

ssh_base=(
  ssh
  -i "${ssh_key}"
  -p "${ssh_port}"
  -o StrictHostKeyChecking=no
  -o UserKnownHostsFile=/dev/null
  -o ConnectTimeout=5
  ci@127.0.0.1
)

for _ in {1..90}; do
  if "${ssh_base[@]}" true >/dev/null 2>&1; then
    break
  fi
  sleep 2
done
"${ssh_base[@]}" true
"${ssh_base[@]}" 'sudo cloud-init status --wait || true'
"${ssh_base[@]}" 'sudo apt-get update'
"${ssh_base[@]}" 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y power-profiles-daemon'

scp -q -i "${ssh_key}" -P "${ssh_port}" \
  -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null \
  "${devd_bin}" ci@127.0.0.1:/home/ci/mains-aegis-devd

"${ssh_base[@]}" 'sudo systemctl restart power-profiles-daemon || true'
"${ssh_base[@]}" 'sudo install -m 0755 /home/ci/mains-aegis-devd /usr/local/bin/mains-aegis-devd'
"${ssh_base[@]}" "printf '%s\n' '${bridge_token}' > /home/ci/mains-aegis-devd.token"
"${ssh_base[@]}" 'sudo env MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=1 nohup /usr/local/bin/mains-aegis-devd bridge-http --bind 0.0.0.0:30080 --allow-lan-bridge --auth-token-file /home/ci/mains-aegis-devd.token > /tmp/mains-aegis-devd.log 2>&1 &'

for _ in {1..60}; do
  if curl -fsS "${auth_header[@]}" "http://127.0.0.1:${api_port}/api/v1/host/power" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
curl -fsS "${auth_header[@]}" "http://127.0.0.1:${api_port}/api/v1/host/power" >/dev/null

curl -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/profile" \
  "${auth_header[@]}" \
  -H 'content-type: application/json' \
  -d '{"profile":"power_saver","dry_run":false}' | jq -e '.ok == true and .dispatch != "not_dispatched"'

profile="$("${ssh_base[@]}" "busctl --system get-property net.hadess.PowerProfiles /net/hadess/PowerProfiles net.hadess.PowerProfiles ActiveProfile")"
case "${profile}" in
  *'"power-saver"'*) ;;
  *)
    echo "Expected Linux guest power profile to become power-saver, got: ${profile}" >&2
    "${ssh_base[@]}" 'cat /tmp/mains-aegis-devd.log' >&2 || true
    exit 1
    ;;
esac

curl -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/profile" \
  "${auth_header[@]}" \
  -H 'content-type: application/json' \
  -d '{"profile":"balanced","dry_run":false}' | jq -e '.ok == true'

set +e
curl --max-time 10 -fsS -X POST "http://127.0.0.1:${api_port}/api/v1/host/power/shutdown" \
  "${auth_header[@]}" \
  -H 'content-type: application/json' \
  -d '{"delay_sec":0,"dry_run":false,"confirm":"shutdown","force":true}'
curl_status=$?
set -e
if [[ "${curl_status}" -ne 0 ]]; then
  echo "Shutdown request did not return before guest started powering off; waiting for VM exit."
fi

for _ in {1..60}; do
  if ! kill -0 "${qemu_pid}" >/dev/null 2>&1; then
    trap - EXIT
    wait "${qemu_pid}" || true
    exit 0
  fi
  sleep 1
done

echo "Linux guest did not power off after devd shutdown command." >&2
"${ssh_base[@]}" 'cat /tmp/mains-aegis-devd.log' >&2 || true
exit 1
