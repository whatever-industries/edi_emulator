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
result_root=${CDI_VMPEG_OUTPUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cdi-vmpeg-switch.XXXXXX")}
if [[ -z ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    trap 'rm -r -- "$result_root"' EXIT
else
    mkdir -p "$result_root"
fi
diagnostics_path="$result_root/fmvdemo-stream-switch.json"
log_path="$result_root/fmvdemo-stream-switch.log"

cd "$repo_root"
EDI_DIAGNOSTIC_EVENT_CAPACITY=512 EDI_DIAGNOSTIC_MILESTONES_ONLY=1 \
    cargo run -q -p cdi-cli --release -- \
    boot "$CDI_SYSTEM_ROM" \
    --instructions "${CDI_VMPEG_SWITCH_INSTRUCTIONS:-1120000000}" \
    --disc "$CDI_FMVDEMO_CUE" \
    --dvc-rom "$CDI_VMPEG_ROM" \
    --click-event '60000000:588,265,1,2000000' \
    --click-event '400000000:520,240,1,2000000' \
    --click-event '650000000:630,250,1,2000000' \
    --click-event '850000000:500,405,1,2000000' \
    --click-event '1000000000:390,468,1,2000000' \
    --diagnostics "$diagnostics_path" \
    --hash | tee "$log_path"

python3 - "$diagnostics_path" <<'PY'
import json
import sys

path = sys.argv[1]
evidence = json.load(open(path, encoding="utf-8"))
stats = evidence["snapshot"]["dvc"]
milestones = [event for event in evidence["events"] if event["kind"] == "dvc-milestone"]
switched = next(
    (event["stats"] for event in milestones if event["stats"]["audio_stream_switch_events"] >= 1),
    None,
)
if switched is None or stats["selected_audio_stream"] != 2:
    raise SystemExit("native FMVDemo Japanese stream selection was not observed")
if stats["decoded_video_frames"] <= switched["decoded_video_frames"] or stats["decoded_audio_frames"] <= switched["decoded_audio_frames"]:
    raise SystemExit("audio/video decoding did not continue after the native stream switch")
if stats["demux_errors"] or stats["video_errors"] or stats["audio_errors"]:
    raise SystemExit("decoder error occurred during the native stream switch")
if stats["audio_underflow_events"]:
    raise SystemExit("audio underflow occurred during the native stream switch")
if stats["audio_concealed_frames"] != 1:
    raise SystemExit(
        "expected exactly one concealed partial Layer-II frame at the stream switch, "
        f"got {stats['audio_concealed_frames']}"
    )
print("native FMVDemo stream switch continued audio/video without decoder errors")
PY

echo "VMPEG stream-switch title scenario passed"
if [[ -n ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    echo "Artifacts retained in $result_root"
fi
