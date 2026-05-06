#!/usr/bin/env python3
"""Generate a Mains Aegis firmware artifact manifest.

The script is intentionally small and deterministic: it records the build
artifact files, hashes them, and emits a manifest consumed by devd and Web.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


FNV_OFFSET_BASIS = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3


def git_value(args: list[str], default: str = "unknown", cwd: Path | None = None) -> str:
    try:
        return (
            subprocess.check_output(["git", *args], cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()
            or default
        )
    except Exception:
        return default


def hash_bytes(value: int, data: bytes) -> int:
    for byte in data:
        value ^= byte
        value = (value * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    return value


def collect_files(root: Path) -> list[Path]:
    if not root.is_dir():
        return []
    return sorted(path for path in root.rglob("*") if path.is_file())


def firmware_source_hash(firmware_dir: Path) -> str:
    files = []
    for name in ["Cargo.toml", "build.rs"]:
        path = firmware_dir / name
        if path.is_file():
            files.append(path)
    files.extend(collect_files(firmware_dir / "src"))
    value = FNV_OFFSET_BASIS
    for path in sorted(files):
        rel = path.relative_to(firmware_dir).as_posix().encode()
        value = hash_bytes(value, rel)
        value = hash_bytes(value, b"\0")
        value = hash_bytes(value, path.read_bytes())
        value = hash_bytes(value, b"\xff")
    return f"{value:016x}"


def stage_artifact_file(kind: str, value: str, out: Path) -> dict[str, object]:
    source = Path(value).resolve()
    dest = out / source.name
    if source != dest:
        shutil.copyfile(source, dest)
    return {
        "kind": kind,
        "path": dest.relative_to(out).as_posix(),
        "sha256": sha256(dest),
        "size": dest.stat().st_size,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--elf", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--name", default="mains-aegis")
    parser.add_argument("--version", default=None)
    parser.add_argument("--profile", default="release")
    parser.add_argument("--features", default="net_http,web_serial")
    parser.add_argument("--bin", default=None)
    parser.add_argument("--defmt-metadata", default=None)
    parser.add_argument("--firmware-dir", default="firmware")
    args = parser.parse_args()

    elf = Path(args.elf).resolve()
    out = Path(args.out).resolve()
    firmware_dir = Path(args.firmware_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)
    git_sha = git_value(["rev-parse", "--short", "HEAD"], cwd=firmware_dir)
    dirty = "dirty" if git_value(
        ["status", "--porcelain", "--untracked-files=no", "--", "src", "Cargo.toml", "build.rs"],
        "",
        cwd=firmware_dir,
    ) else "clean"
    features = [part for part in args.features.split(",") if part]
    src_hash = firmware_source_hash(firmware_dir)
    build_id = f"{git_sha}-{dirty}-{src_hash}"
    artifact_id = f"{args.name}-esp32s3-{args.profile}-{'-'.join(features) or 'default'}-{git_sha}"

    files = []
    for kind, value in [("elf", str(elf)), ("image", args.bin), ("defmt_metadata", args.defmt_metadata)]:
        if not value:
            continue
        files.append(stage_artifact_file(kind, value, out))

    elf_hash = next((item["sha256"] for item in files if item["kind"] == "elf"), None)
    metadata_hash = next((item["sha256"] for item in files if item["kind"] == "defmt_metadata"), None)
    manifest = {
        "artifact_id": artifact_id,
        "name": args.name,
        "version": args.version or os.environ.get("CARGO_PKG_VERSION", "0.1.0"),
        "git_sha": git_sha,
        "git_dirty": dirty,
        "build_id": build_id,
        "target_chip": "esp32s3",
        "profile": args.profile,
        "features": features,
        "protocol": "mains-aegis.cdc.v1",
        "defmt": {
            "enabled": True,
            "encoding": "defmt-espflash",
            "elf_sha256": elf_hash,
            "metadata_sha256": metadata_hash,
        },
        "files": files,
    }
    manifest_path = out / f"{artifact_id}.manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    catalog_path = out / "firmware-catalog.json"
    catalog_path.write_text(json.dumps({"schema_version": 1, "artifacts": [manifest]}, indent=2, sort_keys=True) + "\n")
    sums_path = out / "SHA256SUMS"
    sums_path.write_text("".join(f"{item['sha256']}  {item['path']}\n" for item in files))
    print(manifest_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
