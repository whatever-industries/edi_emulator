# Specification-Driven Debugging Workflow

This is the required workflow whenever a CD-i compatibility problem is
reported. Its purpose is to locate the first inaccurate emulation stage and
make a generalized device correction. Diagnostics are observational: they
must never select crops, delays, status values, or title behavior.

Consult `docs/specification-research.md` before forming a new hardware
hypothesis. Its compliance matrix distinguishes normative specifications,
Philips implementation notes, historical errata, and weaker observational
evidence. A document finding is not itself authorization to change behavior:
it still needs a falsifying test against the affected device boundary.

## Start and reproduce

1. Search `diagnose history` for the title, symptom, and affected components.
2. Create a local ignored incident:

   ```sh
   cargo run -p cdi-cli -- diagnose init \
     --title "Reported title" \
     --symptom "Observed behavior" \
     --expected "Hardware/reference behavior" \
     --component cdic --component mcd212 \
     --disc "/local/path/title.cue"
   ```

   `diagnose init` records the exact current Git commit as
   `reported_revision`. If the report was made against an older or unknown
   build, replace that field with the known commit or explicitly record
   `unknown-pre-REVISION`; never silently assign a guessed commit.

3. Record competing hardware hypotheses in `incident.json`. Every hypothesis
   needs supporting evidence, contradicting evidence, a falsifying test, and
   specification/firmware/reference citations.
4. Reproduce the exact bounded configuration twice. `diagnose run` stores
   stdout/stderr, the final machine snapshot, bounded transition events,
   framebuffer hash, and a payload-free disc inventory:

   ```sh
   cargo run -p cdi-cli -- diagnose run \
     tests-data/local/diagnostics/INCIDENT \
     --rom roms/cdi220b.rom --disc "/local/path/title.cue" \
     --video-standard pal --instructions 1000000
   ```

   A successful run captures evidence but does not by itself prove that the
   symptom occurred. Pass `--symptom-reproduced` only after inspecting the run.
   Doing so records `last_reproduced_revision` and marks the evidence current.
   The inventory's `content_kind`, XA Bridge flag, native `CDI` application
   flag, and PVD identifiers are metadata evidence only and never select a
   host-side playback shortcut.
   Manual acceptance is recorded explicitly:

   ```sh
   cargo run -p cdi-cli -- diagnose verify INCIDENT \
     --accepted --notes "Expected behavior confirmed in the requested pass"
   ```

   This records `last_verified_revision` at the checked-out commit.

5. Compare the two `evidence.json` files. If they diverge before the reported
   symptom, first investigate the nondeterminism.

For a disc-only architecture pass (including CD-i Green Book volumes and ISO
9660/VCD trees), write a payload-free inventory:

```sh
cargo run -p cdi-cli -- disc "/local/path/disc.cue" \
  --inventory-json tests-data/local/diagnostics/disc-inventory.json
```

The inventory records OS-9 modules, root-level VCD `CDI` content, RTF/VCD
sector classifications, White Book entry/PSD topology, and validated MPEG
sequence metadata without extracting media. Compatibility reports pair the
final CDIC LBA with the nearest authored VCD entry, so a native-engine stall
can be compared with the same entry and list topology in later runs.

## Automated compatibility suites

Use `cdi-cli compatibility` for broad, repeatable local-media passes before
manual testing. A suite manifest stays under ignored local storage because it
contains firmware, disc, and optional NVRAM paths. The tracked
`data/compatibility/headless-suite.example.json` documents schema version 1.

```sh
cp data/compatibility/headless-suite.example.json \
  tests-data/local/diagnostics/compatibility/smoke.json
# Edit the copied paths, bounds, scripted inputs, checkpoints, and assertions.
cargo run -p cdi-cli --release -- compatibility run \
  tests-data/local/diagnostics/compatibility/smoke.json
```

Each case runs in a separate process with both an instruction bound and a
wall-clock timeout. Its ignored result directory contains stdout/stderr, final
PNG, 44.1 kHz stereo PCM WAV, deterministic diagnostic evidence, frame/audio
hashes, and a suite summary. A process panic/non-zero exit or timeout is an
unconditional failure. Frame, audio, unique-raster, static-raster, and DVC
error limits are explicit per-case assertions: a static menu is not called a
stall unless the case opts into a maximum identical-raster run.

Review the artifacts and checkpoint manually. Only then promote passing cases:

```sh
cargo run -p cdi-cli -- compatibility promote \
  tests-data/local/diagnostics/compatibility/SUITE/RUN/suite-result.json \
  --accepted
```

Promotion strips all local paths and updates
`data/compatibility/title-matrix.json` by exact disc fingerprint, model,
standard, and DVC configuration. Automated process success alone never claims
that a title reached gameplay. The manifest's checkpoint becomes accepted
compatibility evidence only through the explicit `--accepted` step.

## Smallest distinguishing experiment

Model the disc/display path explicitly:

```text
track → file → real-time record → sector/channel → CDFM/PCL buffer
      → drawmap → MCD212 plane → hardware aperture → frontend
```

Capture the smallest evidence that separates the active hypotheses. For
display faults the comparison stages are:

1. Static source/VDSQ metadata or preview.
2. Disc-delivered bytes and PCL destination.
3. Drawmap after transfer.
4. Individual MCD212 planes.
5. Composed hardware raster and aperture.
6. Frontend-presented output.

RTF previews require context. MPEG dimensions and timestamps can be read from
the stream. DYUV requires the traced absolute start value. RL3/RL7/CLUT needs
the live palette, line pointers, and geometry. A partial update is meaningful
only against the destination drawmap and previous contents. Static previews
are evidence, never automatic crop or presentation input.

For a moving display transition, launch the frontend with
`EDI_DIAGNOSTICS_DIR` and optionally `EDI_DIAGNOSTIC_FIELDS` (1-120; default
4), then press Command-Shift-D at the transition start. The capture contains
consecutive plane RAM, decoded planes, base rasters, composed rasters, and
machine snapshots. Run
`scripts/summarize-display-fields.py <capture-directory>` to produce a
payload-free per-field hash/change summary before interpreting the images.
Longer bursts are diagnostic-only and must not become presentation buffering.

Before changing code, add a failing device-level test. Record the experiment
outcome before changing another subsystem.

## Context-aware experiment memory

Every meaningful experiment records:

- base revision and dirty-diff hash;
- fingerprints for affected components and prerequisites;
- disc fingerprint, model, standard, DVC, scenario, inputs, and timing;
- hypothesis, strategy, symbols, assumptions, expected and actual results;
- improvements, regressions, whether reverted, evidence quality/confidence;
- applicability conditions, invalidating dependency changes, and evidence
  that would justify another attempt.

Use precise outcomes:

- `hypothesis-falsified`: a valid experiment disproved the explanation.
- `implementation-failed`: the code was wrong; the hypothesis may remain.
- `blocked-by-prerequisite`: another subsystem prevented a valid test.
- `regression-causing`: mixed improvement and damage.
- `partial`, `inconclusive`, `confirmed`, or `superseded`.

An equivalent-context repeat is probably redundant, but is never
automatically forbidden. State what new evidence justifies it. A dependency
or upstream-device correction makes prior conclusions candidates for
revalidation. Preserve compact failed-attempt records after resolution; full
temporary patches stay under ignored local diagnostics.

When a material core/frontend checkpoint changes an incident's prerequisites,
set `evidence_status` to `needs-revalidation`, record the checkpoint and reason
in `revalidation_reason`, and retain the original reported/reproduced
revisions. This is deliberately not the same as resolving or invalidating the
bug: the earlier observation becomes potentially stale evidence until rerun.

## Generalized correction and impact

Forbidden fixes include title-name branches, pixel-content cropping,
arbitrary delays, forced status values, direct disc-to-decoder shortcuts, and
constants chosen only until one title passes.

For non-obvious hardware behavior and specification-derived constants, keep a
concise source citation beside the implementation: short document title,
revision when known, and section/table/page. Put edition hashes, conflicting
sources, rejected interpretations, and extended rationale in
`data/compatibility/compliance-matrix.json` or the associated research note;
do not turn implementation comments into a second source ledger.

Before implementation, state:

- hardware hypothesis and its falsifying test;
- supporting and contradictory evidence;
- relevant specification, firmware, or hardware citation;
- how related prior experiments apply to the current prerequisites.

Use this impact matrix for neighboring regressions:

| Changed area | Neighboring checks |
|---|---|
| CPU/timing | scheduling, audio, video, input |
| CDIC | filesystem, CDDA, XA, RTF, DVC, VCD |
| MCD212 | graphics, interlace, cursor, overlays, screenshots, pointer mapping |
| SLAVE | input, buttons, reset, video-standard reporting |
| DVC | MPEG, VCD, A/V synchronization, external video |
| Frontend | host presentation, input, screenshots, audio transport |

## Verification and handoff

`diagnose verify INCIDENT` writes tailored manual steps and neighboring checks.
With `--accepted`, it also records the accepting commit and note.
Every change handoff must say:

- what changed and why;
- prior related experiments and whether their prerequisites still match;
- current experiments and outcomes;
- exact manual steps, expected result, and approximate duration;
- two to five impact-matrix checks and warning signs;
- automated checks completed and remaining unknowns.

Keep the incident `fixed-but-unverified` until the requested manual check is
confirmed. A sanitized record can then be created with:

```sh
cargo run -p cdi-cli -- diagnose promote INCIDENT --reproduced
```

Never promote host paths, ROMs, disc data, extracted media, screenshots, or
audio. Tracked records contain technical metadata and hashes only.
