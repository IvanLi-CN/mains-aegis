#!/usr/bin/env sh
set -eu

prefix="${PREFIX:-/usr/local}"
bindir="${BINDIR:-${prefix}/bin}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

install -d "${bindir}"
install -m 0755 "${script_dir}/bin/mains-aegis" "${bindir}/mains-aegis"
install -m 0755 "${script_dir}/bin/mains-aegis-devd" "${bindir}/mains-aegis-devd"

echo "Installed mains-aegis and mains-aegis-devd to ${bindir}"
