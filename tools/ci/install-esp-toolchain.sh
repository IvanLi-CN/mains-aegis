#!/usr/bin/env bash
# Linux x86_64 CI only. Versions match the espup resolution in the failed run.
set -euo pipefail
[[ "$(uname -sm)" == "Linux x86_64" ]]
task_root="$(mktemp -d "${RUNNER_TEMP:?}/mains-aegis-esp.XXXXXX")"
toolchain_root="$task_root/toolchain"
mkdir -p "$toolchain_root" "$task_root/unpack"
fetch=(python3 tools/ci/download-release-asset.py)
"${fetch[@]}" esp-rs/rust-build v1.97.0.0 \
  rust-1.97.0.0-x86_64-unknown-linux-gnu.tar.xz "$task_root/rust.tar.xz"
"${fetch[@]}" esp-rs/rust-build v1.97.0.0 \
  rust-src-1.97.0.0.tar.xz "$task_root/rust-src.tar.xz"
"${fetch[@]}" espressif/crosstool-NG esp-16.1.0_20260609 \
  xtensa-esp-elf-16.1.0_20260609-x86_64-linux-gnu.tar.xz "$task_root/gcc.tar.xz"
"${fetch[@]}" espressif/llvm-project esp-21.1.3_20260408 \
  libs-clang-esp-21.1.3_20260408-x86_64-linux-gnu.tar.xz "$task_root/clang.tar.xz"
tar -xJf "$task_root/rust.tar.xz" -C "$task_root/unpack"
tar -xJf "$task_root/rust-src.tar.xz" -C "$task_root/unpack"
bash "$task_root/unpack/rust-nightly-x86_64-unknown-linux-gnu/install.sh" \
  --destdir="$toolchain_root" --prefix='' \
  --without=rust-docs-json-preview,rust-docs --disable-ldconfig
bash "$task_root/unpack/rust-src-nightly/install.sh" \
  --destdir="$toolchain_root" --prefix='' --disable-ldconfig
tar -xJf "$task_root/gcc.tar.xz" -C "$toolchain_root"
tar -xJf "$task_root/clang.tar.xz" -C "$toolchain_root"
rustup toolchain link esp "$toolchain_root"
rustup default esp
echo "$toolchain_root/xtensa-esp-elf/bin" >> "${GITHUB_PATH:?}"
echo "LIBCLANG_PATH=$toolchain_root/esp-clang/lib" >> "${GITHUB_ENV:?}"
"$toolchain_root/bin/rustc" --version
"$toolchain_root/xtensa-esp-elf/bin/xtensa-esp32s3-elf-gcc" --version
test -f "$toolchain_root/esp-clang/lib/libclang.so"
