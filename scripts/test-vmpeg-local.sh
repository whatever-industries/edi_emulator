#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

for required in CDI_SYSTEM_ROM CDI_VMPEG_ROM CDI_7TH_GUEST_CUE; do
    if [[ -z ${!required:-} ]]; then
        echo "$required must name a local user-supplied image" >&2
        exit 2
    fi
    if [[ ! -f ${!required} ]]; then
        echo "$required does not exist: ${!required}" >&2
        exit 2
    fi
done

repo_root=$(cd "$(dirname "$0")/.." && pwd)
instructions=${CDI_VMPEG_INSTRUCTIONS:-1100000000}
click_at=${CDI_VMPEG_CLICK_AT:-498000000}

if [[ -n ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    result_root=$CDI_VMPEG_OUTPUT_DIR
    mkdir -p "$result_root"
else
    result_root=$(mktemp -d "${TMPDIR:-/tmp}/cdi-vmpeg-test.XXXXXX")
    trap 'rm -r -- "$result_root"' EXIT
fi

log_path="$result_root/7th-guest.log"
screenshot_path="$result_root/7th-guest.png"

cd "$repo_root"
cargo run -q -p cdi-cli --release -- \
    boot "$CDI_SYSTEM_ROM" \
    --instructions "$instructions" \
    --disc "$CDI_7TH_GUEST_CUE" \
    --dvc-rom "$CDI_VMPEG_ROM" \
    --click 588,265 \
    --click-at "$click_at" \
    --screenshot "$screenshot_path" \
    --hash | tee "$log_path"

if rg -q "avm_play: Still busy from last play|panicked at|errors demux/video/audio [^0]" "$log_path"; then
    echo "VMPEG regression failed; inspect $log_path" >&2
    exit 1
fi

rg -q "errors demux/video/audio 0/0/0" "$log_path"
rg -q "VMPEG end routing: program-end video/audio [1-9][0-9]*/[1-9][0-9]*" "$log_path"

presented=$(sed -nE 's/VMPEG display: ([0-9]+) frames presented.*/\1/p' "$log_path")
audio_samples=$(sed -nE 's/Done: .* ([0-9]+) audio samples/\1/p' "$log_path")
if [[ -z $presented || $presented -eq 0 || -z $audio_samples || $audio_samples -eq 0 ]]; then
    echo "VMPEG regression produced no presented video or audio" >&2
    exit 1
fi

black_hash=$(dd if=/dev/zero bs=1720320 count=1 2>/dev/null | shasum -a 256 | awk '{print $1}')
frame_hash=$(sed -nE 's/Framebuffer SHA-256: ([0-9a-f]+)/\1/p' "$log_path")
if [[ -z $frame_hash || $frame_hash == "$black_hash" ]]; then
    echo "VMPEG regression ended on an all-black framebuffer" >&2
    exit 1
fi

echo "VMPEG local acceptance passed: $presented frames, $audio_samples audio samples"
echo "Final framebuffer: $frame_hash"
if [[ -n ${CDI_VMPEG_OUTPUT_DIR:-} ]]; then
    echo "Artifacts retained in $result_root"
fi
