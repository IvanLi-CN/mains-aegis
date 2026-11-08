#!/usr/bin/env python3
"""Download a pinned official release asset through GitHub's Assets API."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time


def download(repo, tag, name, destination, *, attempts=4):
    metadata = json.loads(subprocess.check_output([
        "gh", "api", f"repos/{repo}/releases/tags/{tag}",
    ], timeout=60))
    assets = [asset for asset in metadata["assets"] if asset["name"] == name]
    if len(assets) != 1:
        raise ValueError(f"Expected exactly one {name} in {repo}@{tag}")
    asset = assets[0]
    digest = asset.get("digest")
    if not isinstance(digest, str) or not digest.startswith("sha256:"):
        raise ValueError(f"Missing SHA256 for {name}; refusing unchecked executable")
    expected = digest.removeprefix("sha256:")
    if len(expected) != 64 or any(c not in "0123456789abcdef" for c in expected):
        raise ValueError("Malformed asset SHA256")
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Failed attempts never replace a previously verified destination.
    with tempfile.TemporaryDirectory(prefix="release-asset-", dir=destination.parent) as temp:
        partial = Path(temp) / "download"
        for attempt in range(attempts):
            try:
                with partial.open("wb") as output:
                    subprocess.run([
                        "gh", "api", "-H", "Accept: application/octet-stream",
                        f"repos/{repo}/releases/assets/{int(asset['id'])}",
                    ], stdout=output, check=True, timeout=600)
                if partial.stat().st_size != asset["size"]:
                    raise ValueError("Release asset size mismatch")
                with partial.open("rb") as source:
                    actual = hashlib.file_digest(source, "sha256").hexdigest()
                if actual != expected:
                    raise ValueError("Release asset SHA256 mismatch")
                partial.replace(destination)
                print(f"Verified {repo}@{tag}/{name}: sha256:{actual}", flush=True)
                return
            except (subprocess.SubprocessError, OSError, ValueError):
                if attempt == attempts - 1:
                    raise
                time.sleep(2 ** attempt)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repo")
    parser.add_argument("tag")
    parser.add_argument("name")
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    download(args.repo, args.tag, args.name, args.destination)
