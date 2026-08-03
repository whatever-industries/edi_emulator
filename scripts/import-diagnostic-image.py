#!/usr/bin/env python3
"""Import screenshots into ignored diagnostics using collision-proof IDs.

Attachment and screenshot filenames are routinely reused.  This helper copies
the bytes immediately to a unique canonical path and records the original name
and content hash beside it.  The imported image remains local and ignored by
Git.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
LOCAL_DIAGNOSTICS = REPO_ROOT / "tests-data" / "local" / "diagnostics"
INCIDENT_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")


def detect_image(data: bytes) -> tuple[str, str]:
    """Return a canonical extension and media type from the file signature."""
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return ".png", "image/png"
    if data.startswith(b"\xff\xd8\xff"):
        return ".jpg", "image/jpeg"
    if data.startswith((b"GIF87a", b"GIF89a")):
        return ".gif", "image/gif"
    if data.startswith(b"BM"):
        return ".bmp", "image/bmp"
    if data.startswith((b"II*\x00", b"MM\x00*")):
        return ".tiff", "image/tiff"
    if len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return ".webp", "image/webp"
    if len(data) >= 12 and data[4:8] == b"ftyp":
        brand = data[8:12]
        if brand in {b"heic", b"heix", b"hevc", b"hevx", b"mif1", b"msf1"}:
            return ".heic", "image/heic"
        if brand in {b"avif", b"avis"}:
            return ".avif", "image/avif"
    raise ValueError("unsupported or unrecognized image format")


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")


def destination_root(output_dir: Path | None, incident: str | None) -> Path:
    if output_dir is not None:
        return output_dir.expanduser().resolve()
    if incident is not None:
        if not INCIDENT_ID.fullmatch(incident):
            raise ValueError(
                "incident must be one local incident ID, not a path "
                "(letters, numbers, '.', '_' and '-' only)"
            )
        return LOCAL_DIAGNOSTICS / incident / "imported-images"
    return LOCAL_DIAGNOSTICS / "imported-images"


def atomic_copy(source: Path, destination: Path) -> None:
    temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}.tmp")
    try:
        shutil.copyfile(source, temporary)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_json(destination: Path, value: dict[str, object]) -> None:
    temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}.tmp")
    try:
        temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def import_image(
    source: Path,
    output_dir: Path,
    incident: str | None,
    label: str | None,
) -> dict[str, object]:
    source = source.expanduser().resolve(strict=True)
    if not source.is_file():
        raise ValueError(f"not a regular file: {source}")

    data = source.read_bytes()
    extension, media_type = detect_image(data)
    digest = hashlib.sha256(data).hexdigest()
    imported_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

    # A UUID makes every observation unique.  The hash suffix makes accidental
    # duplicate content apparent without opening either image.
    evidence_id = (
        f"img-{utc_timestamp()}-{uuid.uuid4().hex[:12]}-{digest[:12]}"
    )
    output_dir.mkdir(parents=True, exist_ok=True)
    image_path = output_dir / f"{evidence_id}{extension}"
    metadata_path = output_dir / f"{evidence_id}.json"
    if image_path.exists() or metadata_path.exists():
        raise RuntimeError(f"generated evidence ID already exists: {evidence_id}")

    metadata: dict[str, object] = {
        "schema_version": 1,
        "evidence_id": evidence_id,
        "kind": "image",
        "imported_at": imported_at,
        "original_filename": source.name,
        "stored_filename": image_path.name,
        "media_type": media_type,
        "byte_length": len(data),
        "sha256": digest,
    }
    if incident is not None:
        metadata["incident"] = incident
    if label is not None:
        metadata["label"] = label

    atomic_copy(source, image_path)
    atomic_json(metadata_path, metadata)
    return {
        **metadata,
        "image_path": str(image_path),
        "metadata_path": str(metadata_path),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Copy screenshots to ignored diagnostics under unique IDs."
    )
    parser.add_argument("images", nargs="+", type=Path, help="image file(s) to import")
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--incident",
        help="local incident ID; stores under that incident's imported-images directory",
    )
    group.add_argument(
        "--output-dir",
        type=Path,
        help="alternate destination (primarily for testing)",
    )
    parser.add_argument("--label", help="short local description of the observation")
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit a JSON array instead of one concise record per image",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        output_dir = destination_root(args.output_dir, args.incident)
        imported = [
            import_image(image, output_dir, args.incident, args.label)
            for image in args.images
        ]
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(imported, indent=2, sort_keys=True))
    else:
        for record in imported:
            print(
                f"{record['evidence_id']}\t{record['image_path']}\t"
                f"sha256:{record['sha256']}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
