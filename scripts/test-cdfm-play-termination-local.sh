#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

for required in CDI_SYSTEM_ROM CDI_FPD805_CUE; do
    if [[ -z ${!required:-} || ! -f ${!required} ]]; then
        echo "$required must name a local user-supplied image" >&2
        exit 2
    fi
done

repo_root=$(cd "$(dirname "$0")/.." && pwd)
result_root=${CDI_CDFM_OUTPUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cdi-cdfm-termination.XXXXXX")}
if [[ -z ${CDI_CDFM_OUTPUT_DIR:-} ]]; then
    trap 'rm -r -- "$result_root"' EXIT
else
    mkdir -p "$result_root"
fi

natural_diagnostics="$result_root/fpd805-bumper-natural.json"
natural_log="$result_root/fpd805-bumper-natural.log"
abort_diagnostics="$result_root/fpd805-bumper-abort.json"
abort_log="$result_root/fpd805-bumper-abort.log"
instructions=${CDI_CDFM_TERMINATION_INSTRUCTIONS:-430000000}

cd "$repo_root"
cargo build -q -p cdi-cli --release

run_case() {
    local diagnostics_path=$1
    local log_path=$2
    shift 2
    EDI_DIAGNOSTIC_EVENT_CAPACITY=32768 \
        target/release/cdi-cli boot "$CDI_SYSTEM_ROM" \
        --instructions "$instructions" \
        --disc "$CDI_FPD805_CUE" \
        --click-event '60000000:588,265,1,2000000' \
        "$@" \
        --diagnostics "$diagnostics_path" \
        --hash | tee "$log_path"
}

# The Philips FPD805 source on this disc supplies both native paths:
#   dev/basecase/bmp_nat/test/bumptest.c
#   dev/basecase/bmp_nat/code/src/bumpanim.c
# bumpanim.c selects channels 0, 15, and 16, routes channel 15 directly to
# audio, and requests three EOR-delimited records. Its two-button handler calls
# ss_abort(); an uninterrupted run instead reaches normal PCB_Rec exhaustion.
run_case "$natural_diagnostics" "$natural_log"
run_case "$abort_diagnostics" "$abort_log" \
    --click-event '360000000:384,280,3,1000000'

python3 - \
    "$natural_diagnostics" "$natural_log" \
    "$abort_diagnostics" "$abort_log" <<'PY'
import json
import sys

natural_path, natural_log_path, abort_path, abort_log_path = sys.argv[1:]
natural = json.load(open(natural_path, encoding="utf-8"))
abort = json.load(open(abort_path, encoding="utf-8"))
natural_log = open(natural_log_path, encoding="utf-8", errors="replace").read()
abort_log = open(abort_log_path, encoding="utf-8", errors="replace").read()

expected_disc = "801a1ace104fcacf7134b44b0d316154d7ffa065"
for name, evidence in (("natural", natural), ("abort", abort)):
    fingerprint = evidence.get("disc", {}).get("fingerprint", {}).get("sha1")
    if fingerprint != expected_disc:
        raise SystemExit(
            f"{name} case used FPD805 fingerprint {fingerprint!r}, expected {expected_disc}"
        )

message = "Bumper was interrupted by the user."
if message in natural_log:
    raise SystemExit("natural case unexpectedly entered the native ss_abort path")
if abort_log.count(message) != 1:
    raise SystemExit("abort case did not report exactly one native ss_abort")

bumppal = next(
    (entry for entry in natural["disc"]["realtime_files"]
     if entry["path"].lower() == "bumppal.rtf"),
    None,
)
if bumppal is None:
    raise SystemExit("FPD805 inventory did not contain bumppal.rtf")
classes = bumppal["sector_classes"]
direct_audio = any(
    item["channel"] == 15
    and item["kind"] == "audio"
    and item["realtime"]
    and item["sectors"] > 0
    for item in classes
)
eor_sectors = sum(item["sectors"] for item in classes if item["eor"])
if not direct_audio or eor_sectors != 3:
    raise SystemExit(
        "bumppal.rtf no longer inventories as channel-15 direct-audio media "
        "with three EOR-delimited records"
    )

expected_file = 0x0100
expected_channels = 0x00018001
expected_audio = 0x8000

def native_timeline(evidence):
    states = [event for event in evidence["events"] if event["kind"] == "cdic-state"]
    active_states = [
        event for event in states
        if event["command"] == 0x2A
        and event["selected_file"] == expected_file
        and event["selected_channels"] == expected_channels
        and event["audio_channel"] == expected_audio
    ]
    if not active_states:
        raise SystemExit("native bumper never established its documented CDIC routing")
    # FPD805 plays several format variants. The scheduled abort lands in the
    # final directly routed play, so compare the last activation in each run.
    active = active_states[-1]
    audio_clear = next(
        (event for event in states
         if event["cycle"] > active["cycle"]
         and event["selected_file"] == expected_file
         and event["selected_channels"] == expected_channels
         and event["audio_channel"] == 0),
        None,
    )
    if audio_clear is None:
        raise SystemExit("native bumper never cleared its direct-audio route")
    update = next(
        (event for event in states
         if event["cycle"] >= audio_clear["cycle"] and event["command"] == 0x2E),
        None,
    )
    if update is None:
        raise SystemExit("native bumper did not issue the CDIC Update command after route clear")
    return active, audio_clear, update

natural_active, natural_clear, natural_update = native_timeline(natural)
abort_active, abort_clear, abort_update = native_timeline(abort)
if abort_clear["cycle"] >= natural_clear["cycle"]:
    raise SystemExit("native abort path did not clear direct-audio routing before natural completion")
if abort_update["cycle"] >= natural_update["cycle"]:
    raise SystemExit("native abort path did not issue CDIC Update before natural completion")
if abort["audio_frames"] >= natural["audio_frames"]:
    raise SystemExit("native abort path did not terminate direct audio before natural completion")
if abort["framebuffer_sha256"] != natural["framebuffer_sha256"]:
    raise SystemExit("native cases did not return to the same final player display")
if abort["snapshot"]["cpu"]["pc"] != natural["snapshot"]["cpu"]["pc"]:
    raise SystemExit("native cases did not return to the same player execution state")

print(
    "native FPD805 CDFM termination passed: "
    f"abort path cleared direct audio at {abort_clear['cycle']} cycles "
    f"versus natural completion at {natural_clear['cycle']}; "
    f"audio frames {abort['audio_frames']} versus {natural['audio_frames']}"
)
PY

echo "CDFM native play-termination scenario passed"
if [[ -n ${CDI_CDFM_OUTPUT_DIR:-} ]]; then
    echo "Artifacts retained in $result_root"
fi
