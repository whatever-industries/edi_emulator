#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

for required in CDI_SYSTEM_ROM CDI_VMPEG_ROM CDI_FMVDEMO_CUE; do
    if [[ -z ${!required:-} || ! -f ${!required} ]]; then
        echo "$required must name a local user-supplied image" >&2
        exit 2
    fi
done

repo_root=$(cd "$(dirname "$0")/.." && pwd)
result_root=${CDI_VMPEG_OUTPUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cdi-vmpeg-pause.XXXXXX")}
if [[ -z ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    trap 'rm -r -- "$result_root"' EXIT
else
    mkdir -p "$result_root"
fi
diagnostics_path="$result_root/fmvdemo-pause-continue.json"
log_path="$result_root/fmvdemo-pause-continue.log"

cd "$repo_root"
# Philips TN 088 documents mv_pause intermittently returning E$NotRdy
# (#246). Bounded retries exercise the native recovery path without teaching
# the emulator or application a title-specific workaround.
EDI_DIAGNOSTIC_EVENT_CAPACITY=512 EDI_DIAGNOSTIC_MILESTONES_ONLY=1 \
    cargo run -q -p cdi-cli --release -- \
    boot "$CDI_SYSTEM_ROM" \
    --instructions "${CDI_VMPEG_PAUSE_INSTRUCTIONS:-1030000000}" \
    --disc "$CDI_FMVDEMO_CUE" \
    --dvc-rom "$CDI_VMPEG_ROM" \
    --click-event '60000000:588,265,1,2000000' \
    --click-event '400000000:520,240,1,2000000' \
    --click-event '650000000:630,250,1,2000000' \
    --click-event '850000000:500,225,1,2000000' \
    --click-event '920000000:475,420,1,2000000' \
    --click-event '940000000:475,420,1,2000000' \
    --click-event '960000000:475,420,1,2000000' \
    --click-event '980000000:475,420,1,2000000' \
    --diagnostics "$diagnostics_path" \
    --hash | tee "$log_path"

python3 - "$diagnostics_path" <<'PY'
import json
import sys

path = sys.argv[1]
evidence = json.load(open(path, encoding="utf-8"))
stats = evidence["snapshot"]["dvc"]
milestones = [event for event in evidence["events"] if event["kind"] == "dvc-milestone"]
paused = next(
    (event["stats"] for event in milestones
     if event["stats"]["pause_events"] >= 1 and event["stats"]["continue_events"] == 0),
    None,
)
if paused is None or stats["continue_events"] < 1:
    raise SystemExit("native FMVDemo pause/continue inputs did not reach both commands")
if stats["demux_errors"] or stats["video_errors"] or stats["audio_errors"]:
    raise SystemExit("decoder error occurred during native pause/continue scenario")
if evidence["snapshot"]["display_provenance"]["mixed_external_generation_fields"]:
    raise SystemExit("one MCD212 field sampled more than one VMPEG picture generation")
if stats["dma_words"] <= paused["dma_words"] or stats["presented_video_frames"] <= paused["presented_video_frames"]:
    raise SystemExit(
        "VMPEG accepted Continue but disc delivery/presentation did not resume; "
        "inspect the retained diagnostic incident"
    )
print("native FMVDemo pause/continue resumed disc DMA and presentation")
PY

echo "VMPEG pause/continue title scenario passed"
if [[ -n ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    echo "Artifacts retained in $result_root"
fi
