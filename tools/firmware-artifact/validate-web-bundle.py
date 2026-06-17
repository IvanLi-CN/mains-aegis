#!/usr/bin/env python3
"""Validate a bundled web/public firmware catalog tree.

This checks the committed fallback bundle for internal consistency without
rebuilding it from the current checkout. It is therefore safe to run on PR
merge refs where the generated current-HEAD artifact identity would differ from
the committed fallback identity.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path: Path) -> Any:
    return json.loads(path.read_text())


def validate_file_entry(bundle_root: Path, artifact_id: str, file_entry: dict[str, Any]) -> str:
    path = file_entry.get("path")
    if not isinstance(path, str) or not path:
        raise SystemExit(f"{artifact_id}: file entry is missing path")
    source = bundle_root / path
    if not source.is_file():
        raise SystemExit(f"{artifact_id}: bundled file not found: {path}")
    expected_sha = file_entry.get("sha256")
    if not isinstance(expected_sha, str) or len(expected_sha) != 64:
        raise SystemExit(f"{artifact_id}: invalid sha256 for {path}")
    actual_sha = sha256(source)
    if actual_sha != expected_sha:
        raise SystemExit(f"{artifact_id}: sha256 mismatch for {path}")
    expected_size = file_entry.get("size")
    if not isinstance(expected_size, int) or expected_size < 0:
        raise SystemExit(f"{artifact_id}: invalid size for {path}")
    actual_size = source.stat().st_size
    if actual_size != expected_size:
        raise SystemExit(f"{artifact_id}: size mismatch for {path}")
    return f"{expected_sha}  {path}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--bundle-root",
        default="web/public/firmware",
        help="Bundled firmware root to validate",
    )
    args = parser.parse_args()

    bundle_root = Path(args.bundle_root).resolve()
    catalog_path = bundle_root / "firmware-catalog.json"
    sums_path = bundle_root / "SHA256SUMS"
    if not catalog_path.is_file():
        raise SystemExit(f"missing bundled catalog: {catalog_path}")
    if not sums_path.is_file():
        raise SystemExit(f"missing bundled SHA256SUMS: {sums_path}")

    catalog = load_json(catalog_path)
    if catalog.get("schema_version") != 1 or not isinstance(catalog.get("artifacts"), list):
        raise SystemExit(f"{catalog_path}: unsupported firmware catalog schema")

    expected_sum_lines: list[str] = []
    seen_artifact_ids: set[str] = set()
    for artifact in catalog["artifacts"]:
        if not isinstance(artifact, dict):
            raise SystemExit(f"{catalog_path}: invalid artifact entry")
        artifact_id = artifact.get("artifact_id")
        if not isinstance(artifact_id, str) or not artifact_id:
            raise SystemExit(f"{catalog_path}: artifact is missing artifact_id")
        if artifact_id in seen_artifact_ids:
            raise SystemExit(f"{catalog_path}: duplicate artifact_id {artifact_id}")
        seen_artifact_ids.add(artifact_id)

        manifest_path = bundle_root / f"{artifact_id}.manifest.json"
        if not manifest_path.is_file():
            raise SystemExit(f"{artifact_id}: missing manifest {manifest_path.name}")
        manifest = load_json(manifest_path)
        if manifest != artifact:
            raise SystemExit(f"{artifact_id}: manifest does not match bundled catalog entry")

        files = artifact.get("files")
        if not isinstance(files, list) or not files:
            raise SystemExit(f"{artifact_id}: artifact has no files")
        has_web_serial_image = False
        for file_entry in files:
            if not isinstance(file_entry, dict):
                raise SystemExit(f"{artifact_id}: invalid file entry")
            expected_sum_lines.append(validate_file_entry(bundle_root, artifact_id, file_entry))
            if (
                file_entry.get("kind") == "image"
                and isinstance(file_entry.get("flash_address"), int)
                and file_entry["flash_address"] >= 0
            ):
                has_web_serial_image = True

        features = artifact.get("features")
        if isinstance(features, list) and "web_serial" in features and not has_web_serial_image:
            raise SystemExit(
                f"{artifact_id}: bundled web_serial artifact must include at least one image file with flash_address",
            )

    actual_sum_lines = [
        line.strip()
        for line in sums_path.read_text().splitlines()
        if line.strip()
    ]
    if actual_sum_lines != expected_sum_lines:
        raise SystemExit(f"{sums_path.name}: content does not match bundled catalog files")

    print(bundle_root)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
