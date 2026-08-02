#!/usr/bin/env python3
"""Summarize a local MCD212 consecutive-field diagnostic capture.

The frontend capture deliberately retains raw plane RAM only in ignored local
diagnostics.  This helper emits payload-free hashes and changed-byte bounds so
an incident can identify whether a transition first changes guest drawmaps or
only the decoded/composed raster.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def change_summary(previous: bytes | None, current: bytes) -> dict[str, int | None]:
    if previous is None:
        return {
            "changed_bytes": None,
            "first_changed_offset": None,
            "last_changed_offset": None,
            "changed_runs": None,
        }
    if len(previous) != len(current):
        raise ValueError("plane RAM sizes differ between captured fields")

    changed = 0
    first = None
    last = None
    runs = 0
    in_run = False
    for offset, (before, after) in enumerate(zip(previous, current, strict=True)):
        differs = before != after
        if differs:
            changed += 1
            first = offset if first is None else first
            last = offset
            if not in_run:
                runs += 1
        in_run = differs
    return {
        "changed_bytes": changed,
        "first_changed_offset": first,
        "last_changed_offset": last,
        "changed_runs": runs,
    }


def summarize(directory: Path) -> dict[str, object]:
    fields = sorted(path for path in directory.glob("field-*") if path.is_dir())
    if not fields:
        raise ValueError(f"no field-* directories found in {directory}")

    previous_planes: dict[str, bytes | None] = {"plane-a": None, "plane-b": None}
    entries = []
    for field in fields:
        snapshot = json.loads((field / "snapshot.json").read_text())
        entry: dict[str, object] = {
            "field": field.name,
            "frame_count": snapshot["mcd212"]["frame_count"],
            "odd_field": snapshot["mcd212"]["geometry"]["odd_field"],
            "geometry": snapshot["mcd212"]["geometry"],
            "registers": {
                key: snapshot["mcd212"][key]
                for key in (
                    "csrw",
                    "csrr",
                    "dcr",
                    "vsr",
                    "ddr",
                    "dcp",
                    "dca",
                    "image_coding_method",
                    "transparency_control",
                    "plane_order",
                    "dyuv_absolute_start",
                )
            },
            "rasters": {
                name: digest(field / name)
                for name in (
                    "plane-a-decoded.png",
                    "plane-b-decoded.png",
                    "base-raster.png",
                    "composed-raster.png",
                )
            },
        }
        planes = {}
        for name in ("plane-a", "plane-b"):
            data = (field / f"{name}-ram.bin").read_bytes()
            planes[name] = {
                "sha256": hashlib.sha256(data).hexdigest(),
                **change_summary(previous_planes[name], data),
            }
            previous_planes[name] = data
        entry["plane_ram"] = planes
        entries.append(entry)

    return {
        "schema_version": 1,
        "capture": directory.name,
        "field_count": len(entries),
        "fields": entries,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path, help="directory containing field-* captures")
    parser.add_argument(
        "--output",
        type=Path,
        help="output JSON (default: <capture>/field-summary.json)",
    )
    args = parser.parse_args()
    output = args.output or args.capture / "field-summary.json"
    output.write_text(json.dumps(summarize(args.capture), indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()
