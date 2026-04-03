#!/usr/bin/env bash
set -euo pipefail

firmware_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
host_target="$(rustc +stable -vV | sed -n 's/^host: //p')"

cargo +stable test --target "$host_target" --manifest-path "$firmware_dir/host-unit-tests/Cargo.toml"
