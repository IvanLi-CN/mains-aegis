#!/usr/bin/env python3
"""Embed firmware catalog artifacts into Web static assets.

The Web app can run without GitHub Releases during local development. This
script takes one or more generated Firmware Catalog files and stages their
manifests and artifact files under web/public/firmware so Vite/devd static
hosting can serve the same artifact contract.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load_catalog(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    if payload.get("schema_version") != 1 or not isinstance(payload.get("artifacts"), list):
        raise SystemExit(f"{path}: unsupported firmware catalog schema")
    return payload


def copy_artifact_files(source_root: Path, public_root: Path, artifact: dict[str, Any]) -> dict[str, Any]:
    artifact_id = artifact.get("artifact_id")
    if not isinstance(artifact_id, str) or not artifact_id:
        raise SystemExit("artifact is missing artifact_id")

    staged = dict(artifact)
    staged_files: list[dict[str, Any]] = []
    artifact_public_dir = public_root / artifact_id
    artifact_public_dir.mkdir(parents=True, exist_ok=True)

    for file_entry in artifact.get("files", []):
        if not isinstance(file_entry, dict):
            raise SystemExit(f"{artifact_id}: invalid file entry")
        source_rel = file_entry.get("path")
        if not isinstance(source_rel, str) or not source_rel:
            raise SystemExit(f"{artifact_id}: file entry is missing path")
        source = (source_root / source_rel).resolve()
        if not source.is_file():
            raise SystemExit(f"{artifact_id}: artifact file not found: {source}")

        dest = artifact_public_dir / source.name
        shutil.copyfile(source, dest)
        staged_file = dict(file_entry)
        staged_file["path"] = f"{artifact_id}/{source.name}"
        staged_file["sha256"] = sha256(dest)
        staged_file["size"] = dest.stat().st_size
        if "flash_address" in file_entry:
            staged_file["flash_address"] = file_entry["flash_address"]
        staged_files.append(staged_file)

    staged["files"] = staged_files
    staged["defmt"] = dict(staged.get("defmt", {}))
    staged["defmt"]["elf_sha256"] = next(
        (item["sha256"] for item in staged_files if item.get("kind") == "elf"),
        staged["defmt"].get("elf_sha256"),
    )
    staged["defmt"]["metadata_sha256"] = next(
        (item["sha256"] for item in staged_files if item.get("kind") == "defmt_metadata"),
        staged["defmt"].get("metadata_sha256"),
    )
    return staged


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog", action="append", required=True, help="Generated firmware-catalog.json")
    parser.add_argument("--out", default="web/public/firmware", help="Web static firmware directory")
    parser.add_argument("--clean", action="store_true", help="Remove existing embedded assets first")
    args = parser.parse_args()

    public_root = Path(args.out).resolve()
    if args.clean and public_root.exists():
        shutil.rmtree(public_root)
    public_root.mkdir(parents=True, exist_ok=True)

    artifacts: list[dict[str, Any]] = []
    seen: set[str] = set()
    for catalog_arg in args.catalog:
        catalog_path = Path(catalog_arg).resolve()
        source_root = catalog_path.parent
        catalog = load_catalog(catalog_path)
        for artifact in catalog["artifacts"]:
            staged = copy_artifact_files(source_root, public_root, artifact)
            artifact_id = staged["artifact_id"]
            if artifact_id in seen:
                raise SystemExit(f"duplicate artifact_id: {artifact_id}")
            seen.add(artifact_id)
            artifacts.append(staged)
            manifest_path = public_root / f"{artifact_id}.manifest.json"
            manifest_path.write_text(json.dumps(staged, indent=2, sort_keys=True) + "\n")

    catalog_payload = {"schema_version": 1, "artifacts": artifacts}
    (public_root / "firmware-catalog.json").write_text(
        json.dumps(catalog_payload, indent=2, sort_keys=True) + "\n",
    )
    (public_root / "SHA256SUMS").write_text(
        "".join(
            f"{item['sha256']}  {item['path']}\n"
            for artifact in artifacts
            for item in artifact.get("files", [])
        ),
    )
    print(public_root / "firmware-catalog.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
