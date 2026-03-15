#!/usr/bin/env bash
set -euo pipefail

firmware_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

cat >"$tmpdir/fan_tests.rs" <<EOF
#![allow(dead_code)]
include!("$firmware_dir/src/fan.rs");
EOF
rustc --edition=2021 --test "$tmpdir/fan_tests.rs" -o "$tmpdir/fan_tests"
"$tmpdir/fan_tests"
