#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("build-catalog-entry.py")
SPEC = importlib.util.spec_from_file_location("build_catalog_entry", SCRIPT_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class StageArtifactFileTests(unittest.TestCase):
    def test_allows_rerun_when_source_is_already_in_output_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir)
            artifact = out / "esp-firmware"
            artifact.write_bytes(b"firmware-bytes")

            entry = MODULE.stage_artifact_file("elf", str(artifact), out)

            self.assertEqual(entry["path"], "esp-firmware")
            self.assertEqual(entry["sha256"], MODULE.sha256(artifact))
            self.assertEqual(entry["size"], artifact.stat().st_size)


if __name__ == "__main__":
    unittest.main()
