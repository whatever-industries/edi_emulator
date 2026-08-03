#!/usr/bin/env python3

from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "import-diagnostic-image.py"
PNG = b"\x89PNG\r\n\x1a\n" + b"project-owned-test-image"


class ImportDiagnosticImageTests(unittest.TestCase):
    def run_import(self, source: Path, output: Path) -> dict[str, object]:
        completed = subprocess.run(
            [
                "python3",
                str(SCRIPT),
                str(source),
                "--output-dir",
                str(output),
                "--label",
                "same displayed name",
                "--json",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return json.loads(completed.stdout)[0]

    def test_repeated_filename_and_content_still_get_unique_ids(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "screenshot.png"
            output = root / "imports"
            source.write_bytes(PNG)

            first = self.run_import(source, output)
            second = self.run_import(source, output)

            self.assertNotEqual(first["evidence_id"], second["evidence_id"])
            self.assertEqual(first["sha256"], second["sha256"])
            self.assertNotEqual(first["image_path"], second["image_path"])
            self.assertEqual(Path(first["image_path"]).read_bytes(), PNG)
            self.assertEqual(Path(second["image_path"]).read_bytes(), PNG)

            metadata = json.loads(Path(first["metadata_path"]).read_text())
            self.assertEqual(metadata["original_filename"], "screenshot.png")
            self.assertEqual(metadata["label"], "same displayed name")

    def test_rejects_non_image_input(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "screenshot.png"
            source.write_text("not actually an image")
            completed = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    str(source),
                    "--output-dir",
                    str(root / "imports"),
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("unrecognized image format", completed.stderr)


if __name__ == "__main__":
    unittest.main()
