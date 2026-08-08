#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import subprocess
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

    def test_uses_explicit_output_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir)
            source = out / "source.elf"
            source.write_bytes(b"firmware-bytes")

            entry = MODULE.stage_artifact_file("elf", str(source), out, "mains-aegis-firmware")

            self.assertEqual(entry["path"], "mains-aegis-firmware")
            self.assertEqual((out / "mains-aegis-firmware").read_bytes(), b"firmware-bytes")


class ManagedOutputTests(unittest.TestCase):
    def test_removes_old_and_current_managed_outputs_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir)
            managed = [
                "esp-firmware",
                "esp-firmware.bin",
                "old-long-name.manifest.json",
                "mains-aegis-firmware",
                "mains-aegis-firmware.bin",
                "firmware-catalog.json",
                "SHA256SUMS",
            ]
            for name in managed:
                (out / name).write_text(name)
            (out / "keep.txt").write_text("keep")

            MODULE.clean_managed_outputs(out)

            self.assertEqual([path.name for path in out.iterdir()], ["keep.txt"])

    def test_preserves_inputs_inside_managed_output_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir)
            source = out / "mains-aegis-firmware"
            source.write_bytes(b"firmware-bytes")

            snapshot, preserved = MODULE.preserve_managed_inputs(out, [source])
            self.addCleanup(snapshot.cleanup)

            self.assertNotEqual(preserved[0], source)
            self.assertEqual(preserved[0].read_bytes(), b"firmware-bytes")


class CatalogGenerationTests(unittest.TestCase):
    def run_generator(
        self, output_stem: str | None, name: str = "mains-aegis"
    ) -> tuple[Path, dict[str, object]]:
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        firmware = root / "firmware"
        firmware.mkdir()
        (firmware / "Cargo.toml").write_text("[package]\nname='fixture'\nversion='0.1.0'\n")
        (firmware / "src").mkdir()
        (firmware / "src" / "main.rs").write_text("fn main() {}\n")
        elf = root / "input-elf"
        image = root / "input-image.bin"
        elf.write_bytes(b"elf")
        image.write_bytes(b"image")
        out = root / "out"
        command = [
            "python3",
            str(SCRIPT_PATH),
            "--elf",
            str(elf),
            "--image",
            f"0x10000:{image}",
            "--out",
            str(out),
            "--firmware-dir",
            str(firmware),
            "--name",
            name,
        ]
        if output_stem:
            command.extend(["--output-stem", output_stem])
        subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
        )
        return out, json.loads((out / "firmware-catalog.json").read_text())

    def test_normal_release_has_stable_short_asset_names(self) -> None:
        out, catalog = self.run_generator("mains-aegis-firmware")

        self.assertEqual(
            sorted(path.name for path in out.iterdir()),
            [
                "SHA256SUMS",
                "firmware-catalog.json",
                "mains-aegis-firmware",
                "mains-aegis-firmware.bin",
                "mains-aegis-firmware.manifest.json",
            ],
        )
        artifact = catalog["artifacts"][0]
        self.assertTrue(artifact["artifact_id"].startswith("mains-aegis-esp32s3-release-net_http-web_serial-"))
        self.assertEqual([entry["path"] for entry in artifact["files"]], ["mains-aegis-firmware", "mains-aegis-firmware.bin"])

    def test_variant_uses_semantic_output_stem(self) -> None:
        out, _ = self.run_generator("mains-aegis-firmware-19v")

        self.assertTrue((out / "mains-aegis-firmware-19v").is_file())
        self.assertTrue((out / "mains-aegis-firmware-19v.bin").is_file())
        self.assertTrue((out / "mains-aegis-firmware-19v.manifest.json").is_file())

    def test_default_output_stem_follows_artifact_name(self) -> None:
        out, _ = self.run_generator(None, name="bq40-comm-tool")

        self.assertTrue((out / "bq40-comm-tool").is_file())
        self.assertTrue((out / "bq40-comm-tool.bin").is_file())
        self.assertTrue((out / "bq40-comm-tool.manifest.json").is_file())

    def test_full_generator_supports_in_place_regeneration(self) -> None:
        out, _ = self.run_generator("mains-aegis-firmware")
        subprocess.run(
            [
                "python3",
                str(SCRIPT_PATH),
                "--elf",
                str(out / "mains-aegis-firmware"),
                "--image",
                f"0x10000:{out / 'mains-aegis-firmware.bin'}",
                "--out",
                str(out),
                "--firmware-dir",
                str(out.parent / "firmware"),
                "--output-stem",
                "mains-aegis-firmware",
            ],
            check=True,
            capture_output=True,
            text=True,
        )

        self.assertEqual(
            sorted(path.name for path in out.iterdir()),
            [
                "SHA256SUMS",
                "firmware-catalog.json",
                "mains-aegis-firmware",
                "mains-aegis-firmware.bin",
                "mains-aegis-firmware.manifest.json",
            ],
        )


if __name__ == "__main__":
    unittest.main()
