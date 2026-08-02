# Core Compatibility Audit

## Purpose

This audit rebuilds the modern application around the exact historical core
behavior that has the strongest manual compatibility evidence, then
reintroduces later device changes in isolated batches. A change is promoted
only when its device-level evidence and neighboring title regressions agree.

## Preserved references

- Current recoverable checkpoint: commit `af3ca1d`.
- Verified historical root tree:
  `2a1d9038b209dc9b0f7c3f05d8065c8ef3c85dcc`.
- Its source archive, executable, hashes, and manual result are stored in the
  ignored local incident directory
  `tests-data/local/diagnostics/7th-guest-pre-game-video-freeze/artifacts/2a1d9038/`.

The historical executable was launched with blank, non-persistent NVRAM, the
PAL CD-i 220 firmware, the supported VMPEG firmware, and The 7th Guest Europe
Disc 1. Manual verification established all of the following independently:

1. The pre-title Philips clip plays.
2. The title MPEG plays and completes.
3. Post-title MPEG 1 plays and completes.
4. A brief black flash replaces an authored transition animation.
5. Post-title MPEG 2 plays and completes.
6. A second brief black flash replaces the authored transition into gameplay.
7. Gameplay begins automatically without audio stutter or looping.

The preserved historical executable has slight audio sizzling. The modern
audit hybrid retains its successful playback behavior and removes that
sizzling.

## First proven regression boundary

The adjacent historical root `ef2f5858` fails the transition. Between
`2a1d9038` and `ef2f5858`, emulation changes are limited to:

- SCC68070 instruction/bus timing in `cpu.rs`, `ea.rs`, and `exec.rs`.
- CDIC sound-map completion signaling.

Controlled historical hybrids established that the SCC68070 timing batch was
sufficient to expose the failure, while the exception/interrupt subset alone
was not. This did not prove the datasheet timing values wrong; it supplied a
reliable first-divergence scenario for the inaccurate device contract
described below.

The Philips documentation pass adds a second constraint to that
reconstruction. TN 94 says disc/audio, video-field, and system-tick time bases
are asynchronous. The CD-i 205 service manual specifies a 30.0000/15.000 MHz
PAL system/CPU clock but a 30.2098 MHz NTSC system clock, matching the
MCD212's 525-TV timing table. The current global 30/15 MHz scheduler cannot
represent that distinction. It remains a player-clock modeling limitation,
but it was not the cause of this transition failure. See
`docs/specification-research.md`.

## Resolved device-contract boundary

Instruction-boundary PCL and register diagnostics located the first causal
divergence. CDIC filled a native PCL and entered its interrupt handler, then
VMPEG preempted that active service on the emulator's separately modeled IN5
before guest PC `$42A594` advanced the PCL producer pointer. VMPEG released
the PCL; the resumed CDIC handler skipped the pointer update; the next sector
then overwrote the same ring position and damaged MPEG ordering.

The Philips CD-i 220 service schematic and signal glossary resolve the
hardware ambiguity: the FMV extension and CDIC share a daisy-chained IN4, and
IN5 is unused. The corrected model latches the active IN4 owner until its
request releases and routes interrupt acknowledge to that owner's programmed
vector. With section-6.2 CPU timing enabled, the identical
550-million-instruction run advances from 18 decoded video frames and one
video error to 768 decoded/744 presented frames, two completed program ends,
and zero demux/video/audio errors. Removing an unrelated DMA0 experiment and
repeating gives identical results.

## Classification

### Preserve independently

- Modern frontend, library, controller, settings, and packaging work.
- Photo CD support and Store-only ZIP loading.
- Diagnostic snapshots, incident history, and disc inventory.
- Firmware/model identification and exact-disc PAL/NTSC recommendations.

These do not alter SCC68070/VMPEG event ordering when kept outside the
emulation loop.

### Revalidate as core batches

- SLAVE input, reset, and live media-change behavior.
- MCD212 geometry, interlace, cursor, and color-range corrections.
- CDIC sound-map and real-time audio completion.
- MPEG decoder reference-frame and error-recovery changes.
- Persistent NVRAM and generated-CSD invalidation.

Each batch has useful evidence, but must be retested against the preserved
seven-stage transition and its neighboring titles.

### Deferred or separately scoped

- DMAREQ2 burst/cycle-steal pacing added during the failed transition
  investigation.
- VMPEG completion changes whose synthetic tests pass but did not restore the
  reported sequence.

The datasheet SCC68070 timing batch is no longer quarantined: its four
table-driven checks are active and the shared-IN4 correction preserves the
bounded title transition. The items above remain contextual evidence, not
prohibitions on a later experiment with changed prerequisites.

## Regression gates

Every promoted core batch must keep:

- The 7th Guest seven-stage sequence above.
- CD Shoot's hovered language flag and skeets at their intended cadence.
- Earth Command sound-map completion without looping.
- The Naked Gun copyright-to-menu transition.
- Alien Gate pointer input and firing audio.
- One PAL and one NTSC base-graphics title.
- Formatting, zero-warning clippy, workspace tests, and the 118,187-vector
  Harte release suite.

The two missing transition animations remain tracked separately and must not
be confused with the resolved post-title playback and audio regressions.

## Manual neighboring-title validation

Manual validation on 2026-07-25 checked the committed modern audit baseline
(`18beb7f`) against the highest-risk neighboring regressions:

- Earth Command's menu audio terminates instead of looping.
- The Naked Gun 2 1/2 releases its copyright clip and reaches the disc menu.
- Alien Gate accepts pointer input and its firing audio does not repeat or
  loop.
- Merlin's Apprentice (Europe) works well as the PAL/50 Hz base-graphics
  check.
- The Apprentice (USA) works well as the NTSC/60 Hz base-graphics check.

These results validate the retained CDIC sound-map completion, masked VMPEG
VSYNC/release path, and SLAVE/input behavior under the legacy SCC68070 timing
baseline. Together with the accepted seven-stage The 7th Guest run, every
manual neighboring-title gate required before timing/scheduling
reconstruction now passes.

Manual validation on 2026-07-26 rechecked Earth Command audio termination and
Alien Gate gameplay/firing audio at revision `ac20dcd`. Both played without
the historical looping fault. Alien Gate's separate missing-HUD edge report
is display-only and is tracked in
`data/compatibility/incidents/alien-gate-hud-lower-edge-missing.json`.

Real-hardware footage reviewed on 2026-08-01 establishes a more precise Alien
Gate audio oracle: bullet effects are present earlier, then stop after the bat
enemies fly. E-Di matches that sequence. The earlier unbounded "no bullet
sound" observation may have been made after this authored boundary and must
not be treated as a regression without a pre-/post-bat checkpoint. See
`data/compatibility/incidents/alien-gate-post-bat-bullet-silence.json` and the
hardware reference at <https://www.youtube.com/watch?v=Vj0GFhxTsi0>.

Later on 2026-07-26, the corrected shared-IN4 build passed the deterministic
550-million-instruction The 7th Guest transition checkpoint twice under the
section-6.2 timing model, including one run with the unrelated DMA0 experiment
removed. A clean-NVRAM frontend pass on 2026-07-29 confirmed that the pre-title
clip, title MPEG, both post-title MPEG stages, transition into the stairwell,
automatic gameplay entry, and gameplay audio all proceed. The neighboring
Earth Command, Naked Gun, Alien Gate, Merlin's Apprentice, and The Apprentice
checks had already passed. This closes the shared-IN4 scheduling
regression. Three brief black intervals and one or two early Philips-logo
audio hits remain separate presentation/audio incidents.

## Mono-I bus-error and rate calibration evidence

External Mono-I development feedback on 2026-07-31 narrows the slow-boot
hypothesis: the known BERR-sensitive probes are outside the decoded memory
map, and correct faults shorten the firmware's early memory checks. The source
has not checked every hole, access width, or direction, so this is not evidence
for making every open-bus access fault unconditionally.

A read-only E-Di trace at revision `36b46f7` confirmed the cost. During the
first 20,000,000 instructions of a PAL cdi220b firmware-only boot, the old
open-bus path handled 9,605,190 byte reads at 9,075,906 distinct addresses
from `$00500000` through `$00FF8003`.

The SCC68070 April 1993 product specification sections 5.9-5.10 and Figures
14-16 now constrain the implementation: vector 2 stacks the 17-word format-F
frame, including SSW access metadata and TPF fault address, and SSW.RR controls
whether long-frame `RTE` retries the failed cycle. Focused tests cover frame
layout, RR suppression, access metadata, and the boundary between known BERR
ranges and an unverified open-bus hole. Only the externally verified Mono-I
absent-memory ranges `$080000-$1fffff` and `$500000-$cfffff` fault. At an exact
30,000,000-instruction comparison, revision `3bfb46d` remained cyan after
330,734,670 cycles; the corrected worktree reached the player shell after
159,555,961 cycles. The full address/access matrix remains open, so other
holes must not be promoted to bus errors without hardware evidence.

CPU-rate calibration is a separate question. Green Book does not prescribe a
CPU clock, while contemporary software often assumes the de facto Mono-I
SCC68070 rate. The external comparison implementation is itself reported to
run slightly fast, so another emulator or FPGA core is not a sufficient timing
oracle. Use one field-timed benchmark on a physical Mono-I player and E-Di to
compare guest work, timer deltas, interrupt cadence, and bus waits. Do not
compensate with host sleep or title-specific throttling.
