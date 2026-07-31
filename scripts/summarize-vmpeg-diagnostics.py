#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Create payload-free, per-play VMPEG timing and transition summaries."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


COUNTER_FIELDS = (
    "decoded_video_frames",
    "decoded_audio_frames",
    "presented_video_frames",
    "video_program_end_events",
    "audio_program_end_events",
    "video_underflow_events",
    "audio_underflow_events",
    "demux_errors",
    "video_errors",
    "audio_errors",
)


def counter_delta(end: dict[str, int], start: dict[str, int], field: str) -> int:
    return int(end.get(field, 0)) - int(start.get(field, 0))


def summarize(evidence: dict[str, Any], video_rate: float) -> dict[str, Any]:
    events = evidence.get("events", [])
    milestones = [
        event for event in events if event.get("kind") == "dvc-milestone"
    ]
    starts: list[dict[str, Any]] = []
    prior_plays = 0
    for event in milestones:
        play_events = int(event["stats"].get("play_events", 0))
        if play_events > prior_plays:
            starts.append(event)
            prior_plays = play_events

    snapshot = evidence["snapshot"]
    final_stats = snapshot.get("dvc") or {}
    final_registers = snapshot.get("dvc_registers") or {}
    final_point = {
        "cycle": int(snapshot["cpu"]["cycles"]),
        "dclk": int(final_registers.get("dclk", 0)),
        "stats": final_stats,
    }

    plays: list[dict[str, Any]] = []
    for index, start in enumerate(starts):
        end = starts[index + 1] if index + 1 < len(starts) else final_point
        start_cycle = int(start["cycle"])
        end_cycle = int(end["cycle"])
        start_dclk = int(start["dclk"])
        end_dclk = int(end["dclk"])
        dclk_delta = (end_dclk - start_dclk) & 0xFFFF_FFFF
        selected_milestones = [
            event
            for event in milestones
            if start_cycle <= int(event["cycle"]) < end_cycle
        ]
        raster_hasher = hashlib.sha256()
        for milestone in selected_milestones:
            raster_hasher.update(
                int(milestone.get("raster_hash", 0)).to_bytes(8, "big")
            )

        start_stats = start["stats"]
        end_stats = end["stats"]
        deltas = {
            field: counter_delta(end_stats, start_stats, field)
            for field in COUNTER_FIELDS
        }
        video_seconds = deltas["presented_video_frames"] / video_rate
        audio_seconds = deltas["decoded_audio_frames"] * 1152 / 44_100
        plays.append(
            {
                "play": int(start_stats["play_events"]),
                "start_cycle": start_cycle,
                "end_cycle": end_cycle,
                "start_dclk": start_dclk,
                "end_dclk": end_dclk,
                "duration_dclk": dclk_delta,
                "duration_seconds": dclk_delta / 45_000,
                **deltas,
                # These are independent throughput estimates, not an A/V sync
                # verdict: a play epoch can legitimately continue after one
                # stream has ended or external video has been hidden.
                "presented_video_duration_seconds_estimate": video_seconds,
                "decoded_audio_duration_seconds_estimate": audio_seconds,
                "raster_milestones": len(selected_milestones),
                "milestone_raster_sequence_sha256": raster_hasher.hexdigest(),
            }
        )

    return {
        "schema_version": 2,
        "source_schema_version": evidence.get("schema_version"),
        "video_rate": video_rate,
        "play_count": len(plays),
        "plays": plays,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--video-rate", type=float, default=25.0)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.video_rate <= 0:
        parser.error("--video-rate must be positive")

    evidence = json.loads(args.evidence.read_text())
    summary = summarize(evidence, args.video_rate)
    rendered = json.dumps(summary, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)

    print(f"VMPEG transition summary: {summary['play_count']} play(s)")
    for play in summary["plays"]:
        print(
            "  play {play}: {seconds:.3f}s, video {video} presented, "
            "audio {audio} frames, errors {demux}/{verr}/{aerr}, "
            "raster {raster}".format(
                play=play["play"],
                seconds=play["duration_seconds"],
                video=play["presented_video_frames"],
                audio=play["decoded_audio_frames"],
                demux=play["demux_errors"],
                verr=play["video_errors"],
                aerr=play["audio_errors"],
                raster=play["milestone_raster_sequence_sha256"],
            )
        )


if __name__ == "__main__":
    main()
