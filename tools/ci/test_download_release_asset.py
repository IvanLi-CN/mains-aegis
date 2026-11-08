import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "release_asset", Path(__file__).with_name("download-release-asset.py")
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class DownloadTests(unittest.TestCase):
    payload = b"verified official executable archive"

    def metadata(self):
        return {"assets": [{"id": 123, "name": "tool.zip", "size": len(self.payload),
                            "digest": "sha256:" + hashlib.sha256(self.payload).hexdigest()}]}

    def exercise(self, bodies, metadata=None, initial=None):
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / "tool.zip"
            if initial is not None:
                output.write_bytes(initial)
            responses = iter(bodies)

            def run(command, *, stdout, check, timeout):
                self.assertEqual(command, ["gh", "api", "-H", "Accept: application/octet-stream",
                                          "repos/owner/tool/releases/assets/123"])
                body = next(responses)
                if isinstance(body, Exception):
                    raise body
                stdout.write(body)

            with patch.object(module.subprocess, "check_output", return_value=json.dumps(
                    metadata if metadata is not None else self.metadata()).encode()), \
                    patch.object(module.subprocess, "run", side_effect=run) as calls, \
                    patch.object(module.time, "sleep"):
                try:
                    module.download("owner/tool", "v1", "tool.zip", output, attempts=2)
                except (ValueError, subprocess.SubprocessError):
                    self.assertEqual(output.read_bytes() if output.exists() else None, initial)
                    self.assertEqual(list(Path(root).glob("release-asset-*")), [])
                    raise
                self.assertEqual(output.read_bytes(), self.payload)
                return calls.call_count

    def test_success_uses_assets_api(self):
        self.assertEqual(self.exercise([self.payload]), 1)

    def test_http_504_then_success(self):
        error = subprocess.CalledProcessError(1, "gh")
        self.assertEqual(self.exercise([error, self.payload]), 2)

    def test_truncated_html_is_retried_and_never_published(self):
        self.assertEqual(self.exercise([b"<h1>504</h1>", self.payload]), 2)

    def test_equal_length_corruption_rejected_preserving_existing_file(self):
        with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
            self.exercise([b"x" * len(self.payload)] * 2, initial=b"previous verified file")

    def test_partial_timeout_is_retried(self):
        self.assertEqual(self.exercise([subprocess.TimeoutExpired("gh", 600), self.payload]), 2)

    def test_persistent_truncation_leaves_no_destination(self):
        with self.assertRaisesRegex(ValueError, "size mismatch"):
            self.exercise([b"504"] * 2)

    def test_missing_digest_refuses_download(self):
        metadata = self.metadata()
        metadata["assets"][0]["digest"] = None
        with self.assertRaisesRegex(ValueError, "Missing SHA256"):
            self.exercise([], metadata)

    def test_missing_or_duplicate_asset_refuses_download(self):
        for assets in [[], self.metadata()["assets"] * 2]:
            with self.subTest(assets=assets), self.assertRaisesRegex(ValueError, "exactly one"):
                self.exercise([], {"assets": assets})


if __name__ == "__main__":
    unittest.main()
