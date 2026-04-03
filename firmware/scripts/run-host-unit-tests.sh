#!/usr/bin/env bash
set -euo pipefail

firmware_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo test --manifest-path "$firmware_dir/host-unit-tests/Cargo.toml"
