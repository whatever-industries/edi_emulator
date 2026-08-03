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
result_root=${CDI_CDFM_PCB_REC_OUTPUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cdi-cdfm-pcb-rec.XXXXXX")}
if [[ -z ${CDI_CDFM_PCB_REC_OUTPUT_DIR:-} ]]; then
    trap 'rm -r -- "$result_root"' EXIT
else
    mkdir -p "$result_root"
fi
instructions=${CDI_CDFM_PCB_REC_INSTRUCTIONS:-430000000}
if [[ ! $instructions =~ ^[0-9]+$ ]] || (( instructions < 341000000 )); then
    echo "CDI_CDFM_PCB_REC_INSTRUCTIONS must be at least 341000000" >&2
    exit 2
fi

cd "$repo_root"
cargo build -q -p cdi-cli --release

python3 - "$CDI_SYSTEM_ROM" <<'PY'
import hashlib
import pathlib
import sys

expected = "fd123e66beadaf844cb220a44166ea33f9fd0d64bafb9e6399febff445429db2"
actual = hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest()
if actual != expected:
    raise SystemExit(f"system ROM SHA-256 {actual} does not match the exact fixture ROM")
PY

inventory="$result_root/fpd805-inventory.json"
target/release/cdi-cli disc "$CDI_FPD805_CUE" --inventory-json "$inventory" >/dev/null
python3 - "$inventory" <<'PY'
import json
import sys

expected = "801a1ace104fcacf7134b44b0d316154d7ffa065"
with open(sys.argv[1], encoding="utf-8") as stream:
    actual = json.load(stream).get("fingerprint", {}).get("sha1")
if actual != expected:
    raise SystemExit(f"disc fingerprint {actual!r} does not match exact FPD805 fixture")
PY

run_case() {
    local name=$1
    shift
    EDI_DIAGNOSTIC_EVENT_CAPACITY=32768 \
        target/release/cdi-cli boot "$CDI_SYSTEM_ROM" \
        --instructions "$instructions" \
        --disc "$CDI_FPD805_CUE" \
        --click-event '60000000:588,265,1,2000000' \
        "$@" \
        --diagnostics "$result_root/$name.json" \
        --hash | tee "$result_root/$name.log"
}

# These addresses belong only to the exact FPD805 fingerprint asserted below.
# The native bmp_nat allocation is deterministic: its final bumper PCB is at
# $27ada4, channel 15 is direct XA audio, and its Audio CIL[15] is initially
# null. Restricting PCB_Chan to channel 15 intentionally stops the animation's
# video feeds, hence its harmless "Bumper length exceeded" debug messages.
#
# TN 085.1's portable workaround is to clear PCB_AChan immediately before
# PCB_Rec. It says the selected audio CIL may remain null or point to RAM. The
# nonzero case reuses an inactive one-sector PCL and Form-2-sized video buffer
# after removing channel 16 from PCB_Chan; no commercial payload is retained.
common=(--memory-write-event '320000000:0x0027adac:00008000')

run_case direct-audio \
    "${common[@]}" \
    --memory-write-event '340000000:0x0027ada8:00000000'

run_case workaround-zero-cil \
    "${common[@]}" \
    --memory-write-event '340000000:0x0027adb0:0000' \
    --memory-write-event '340000000:0x0027ada8:00000000'

run_case workaround-nonzero-cil \
    "${common[@]}" \
    --memory-write-event '340000000:0x0027aefe:0000000000000027aefe0005cee00000000100000000000000000000' \
    --memory-write-event '340000000:0x0027aefa:0027aefe' \
    --memory-write-event '340000000:0x0027adb0:0000' \
    --memory-write-event '340000000:0x0027ada8:00000000'

python3 - "$result_root" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
names = ("direct-audio", "workaround-zero-cil", "workaround-nonzero-cil")
evidence = {
    name: json.load(open(root / f"{name}.json", encoding="utf-8"))
    for name in names
}
logs = {
    name: (root / f"{name}.log").read_text(encoding="utf-8", errors="replace")
    for name in names
}

expected_disc = "801a1ace104fcacf7134b44b0d316154d7ffa065"
for name in names:
    fingerprint = evidence[name].get("disc", {}).get("fingerprint", {}).get("sha1")
    if fingerprint != expected_disc:
        raise SystemExit(
            f"{name} used FPD805 fingerprint {fingerprint!r}, expected {expected_disc}"
        )
    if "Bumper was interrupted by the user." in logs[name]:
        raise SystemExit(f"{name} entered the native error/abort path")

def diagnostic_hash(data):
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value

def patch_index(patches, address, before, after):
    byte_count = len(after)
    matches = [
        index for index, event in enumerate(patches)
        if event["memory_address"] == address and event["bytes"] == byte_count
    ]
    if len(matches) != 1:
        raise SystemExit(
            f"expected exactly one {byte_count}-byte diagnostic patch at {address:#010x}"
        )
    index = matches[0]
    event = patches[index]
    if before is not None and event["before_hash"] != diagnostic_hash(before):
        raise SystemExit(f"unexpected live value before patch at {address:#010x}")
    if event["after_hash"] != diagnostic_hash(after):
        raise SystemExit(f"unexpected installed value after patch at {address:#010x}")
    return index

def timeline(name, item):
    events = item["events"]
    patches = [event for event in events if event["kind"] == "diagnostic-ram-patch"]
    channel_index = patch_index(
        patches,
        0x0027ADAC,
        bytes.fromhex("00018001"),
        bytes.fromhex("00008000"),
    )
    channel_patch = patches[channel_index]
    rec_index = patch_index(
        patches,
        0x0027ADA8,
        bytes.fromhex("00000001"),
        bytes.fromhex("00000000"),
    )
    rec_patch = patches[rec_index]
    if rec_patch["changed_bytes"] != 1:
        raise SystemExit("PCB_Rec was not changed from one to zero exactly once")
    achan = [
        index for index, event in enumerate(patches)
        if event["memory_address"] == 0x0027ADB0 and event["bytes"] == 2
    ]
    if name == "direct-audio":
        if achan:
            raise SystemExit("direct-audio case unexpectedly patched PCB_AChan")
    else:
        achan_index = patch_index(
            patches,
            0x0027ADB0,
            bytes.fromhex("8000"),
            bytes.fromhex("0000"),
        )
        if achan != [rec_index - 1] or achan != [achan_index]:
            raise SystemExit("workaround did not clear PCB_AChan immediately before PCB_Rec")
    if name == "workaround-nonzero-cil":
        cil_index = patch_index(
            patches,
            0x0027AEFA,
            bytes.fromhex("00000000"),
            bytes.fromhex("0027aefe"),
        )
        pcl_index = patch_index(
            patches,
            0x0027AEFE,
            None,
            bytes.fromhex(
                "0000000000000027aefe0005cee00000000100000000000000000000"
            ),
        )
        if not pcl_index < cil_index < rec_index - 1:
            raise SystemExit(
                "nonzero-CIL PCL was not initialized, published, and routed in order"
            )
        if channel_patch["cycle"] >= patches[pcl_index]["cycle"]:
            raise SystemExit("nonzero-CIL PCL was published before channel 16 became inactive")
    staged = any(
        event["kind"] == "cdic-state"
        and event["cycle"] < rec_patch["cycle"]
        and event["selected_channels"] == 0x8000
        and event["audio_channel"] == 0x8000
        for event in events
    )
    if not staged:
        raise SystemExit("fixture did not isolate channel-15 direct audio before PCB_Rec clear")
    selected_sector = next(
        (event for event in events
         if event["kind"] == "cdic-state"
         and event["cycle"] > rec_patch["cycle"]
         and event["selected_channels"] == 0x8000
         and event["interrupt_asserted"]
         and event["x_buffer"] & 0x8000),
        None,
    )
    if selected_sector is None:
        raise SystemExit("no selected sector arrived after PCB_Rec clear")
    route_clear = next(
        (event for event in events
         if event["kind"] == "cdic-state"
         and event["cycle"] >= selected_sector["cycle"]
         and event["audio_channel"] == 0),
        None,
    )
    if route_clear is None:
        raise SystemExit("CDFM did not end the direct-audio route")
    next_sector = next(
        (event for event in events
         if event["kind"] == "disc-position"
         and event["cycle"] > selected_sector["cycle"]),
        None,
    )
    if next_sector is None or route_clear["cycle"] >= next_sector["cycle"]:
        raise SystemExit("PCB_Rec clear was not recognized within the selected-sector handler")
    return rec_patch, selected_sector, route_clear

timelines = {name: timeline(name, evidence[name]) for name in names}
if len({timelines[name][0]["cycle"] for name in names}) != 1:
    raise SystemExit("PCB_Rec was not cleared at the same point in all three cases")
if len({timelines[name][1]["cycle"] for name in names}) != 1:
    raise SystemExit("the three cases did not recognize the same next selected sector")
zero = timelines["workaround-zero-cil"]
nonzero = timelines["workaround-nonzero-cil"]
if tuple(event["cycle"] for event in zero) != tuple(event["cycle"] for event in nonzero):
    raise SystemExit("zero and nonzero audio-CIL workaround timelines diverged")
for field in ("audio_frames", "framebuffer_sha256"):
    if len({evidence[name][field] for name in names}) != 1:
        raise SystemExit(f"PCB_Rec cases differ in {field}")
if len({evidence[name]["snapshot"]["cpu"]["pc"] for name in names}) != 1:
    raise SystemExit("PCB_Rec cases did not return to the same player state")

for name in names:
    rec_patch, selected_sector, route_clear = timelines[name]
    print(
        f"{name}: PCB_Rec cleared at {rec_patch['cycle']}, "
        f"selected sector at {selected_sector['cycle']}, "
        f"audio route ended at {route_clear['cycle']}"
    )
print("native FPD805 PCB_Rec selected-sector matrix passed")
PY

echo "CDFM PCB_Rec scenario passed"
if [[ -n ${CDI_CDFM_PCB_REC_OUTPUT_DIR:-} ]]; then
    echo "Artifacts retained in $result_root"
fi
