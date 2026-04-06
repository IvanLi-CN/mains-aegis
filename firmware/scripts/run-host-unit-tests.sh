#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
firmware_dir="$repo_root/firmware"
host_target="$(rustc +stable -vV | sed -n 's/^host: //p')"

python3 "$firmware_dir/scripts/audit-host-tests.py"

manifests=(
  "$firmware_dir/host-unit-tests/Cargo.toml"
  "$repo_root/host-tests/runtime-audio-recovery/Cargo.toml"
)

for manifest in "${manifests[@]}"; do
  cargo +stable test --target "$host_target" --manifest-path "$manifest"
done
