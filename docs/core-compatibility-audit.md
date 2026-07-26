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

Controlled historical hybrids established that the SCC68070 timing batch is
sufficient to expose the failure, while the exception/interrupt subset alone
is not. This does not prove the datasheet timing values are wrong. It proves
that the rest of the emulator does not remain behaviorally correct when all
device time is advanced in instruction-sized batches under those values.

The Philips documentation pass adds a second constraint to that
reconstruction. TN 94 says disc/audio, video-field, and system-tick time bases
are asynchronous. The CD-i 205 service manual specifies a 30.0000/15.000 MHz
PAL system/CPU clock but a 30.2098 MHz NTSC system clock, matching the
MCD212's 525-TV timing table. The current global 30/15 MHz scheduler cannot
represent that distinction. This strengthens the event-interleaving
hypothesis; it does not justify changing an isolated clock constant while
devices still advance in instruction-sized batches. See
`docs/specification-research.md`.

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

### Quarantine pending reconstruction

- Datasheet SCC68070 instruction timing when devices advance only after a
  complete instruction.
- DMAREQ2 burst/cycle-steal pacing added during the failed transition
  investigation.
- VMPEG completion changes whose synthetic tests pass but did not restore the
  reported sequence.

Quarantine means the change remains preserved in `af3ca1d`; it is not evidence
that the underlying hardware hypothesis is false.

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

On 2026-07-25 the user manually verified the committed modern audit baseline
(`18beb7f`) against the three highest-risk neighboring regressions:

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

On 2026-07-26 the user revalidated Earth Command audio termination and Alien
Gate gameplay/firing audio on the `ac20dcd` diagnostic working tree. Both
played without the historical looping fault. Alien Gate's separate missing-HUD
edge report is display-only and is tracked in
`data/compatibility/incidents/alien-gate-hud-lower-edge-missing.json`.
