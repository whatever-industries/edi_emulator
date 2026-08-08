# E-Di: Emulator Disc Interactive TODO

This is the short project roadmap. Sanitized compatibility evidence belongs in
`data/compatibility/incidents/`; full logs, captures, media paths, and temporary
patches belong in the ignored `tests-data/local/diagnostics/`.

## Evidence checkpoint

- [ ] Revalidate unresolved compatibility reports against `ac20dcd` or a later
  clean commit before presenting them as current behavior. Reports and
  experiments from earlier builds are potentially stale: they remain useful
  evidence, but may or may not apply after the 2026-07-25 cold-power-cycle,
  persistent-NVRAM, timing, DVC, and frontend changes.

## External technical-review follow-ups (2026-07-31)

- [ ] Re-review MAME, MiSTer CD-i, and CeDImu when
  `scripts/check-external-project-updates.py` reports a revision newer than the
  last substantively reviewed revision in
  `data/external-project-reviews.json`. Record the date, exact revision,
  subsystem scope, findings, and resulting tests; an observed upstream HEAD is
  not itself a completed review.
- [ ] Clarify the player-generation roadmap before adding another board.
  E-Di's current CD-i 200/210/220 implementation is already **Mono-I**. Inventory
  the device maps, interrupt wiring, SLAVE/IKAT behavior, storage, firmware, and
  DVC interfaces of Mono-II, Mono-III/Roboco, and Mono-IV, then select the next
  model explicitly. Keep the current M4 boundary of CD-i 490/Mono-IV + IMPEG
  unless evidence justifies prioritizing a different generation.
- [ ] Finish portable frontend audio after that retest: add deterministic
  44.1 kHz-to-host-rate conversion where necessary, preserve channel mapping,
  and keep an audio-start failure persistently visible instead of allowing
  silent video playback.
- [ ] Complete the Mono-I address-response matrix before extending BERR beyond
  the two verified ranges. Capture PC/function-code/size/direction on hardware
  for representative holes, byte/word/long transfers, writes, and user versus
  supervisor accesses. Keep the current implementation narrow rather than
  converting every unmapped page into a blanket fault.
- [ ] Reproduce the Debian report that Plunderball advances too quickly and
  Hotel Mario flickers on lower layers. Use Mono-I/SCC68070 as the hardware
  baseline: Green Book does not mandate a CPU speed, while real titles commonly
  depend on the near-universal 68070 player rate. Compare hardware-measured
  cycles per field, instruction/bus timing, mapped-device wait states, and BERR
  probes before changing pacing. Do not use host sleep or a title-specific
  throttle to compensate. The reporter states that Mario must not flicker;
  macOS now reproduces the lower-half flicker, and manual testing confirmed it is
  also present in E-Di 0.1.0, so treat it as longstanding rather than a recent
  regression. Prepare a
  small field-timed benchmark that can run unchanged on a physical Mono-I
  player and E-Di; another emulator or FPGA core is not sufficient calibration.
- [ ] Complete the physical output-control side of SLAVE audio: trace commands
  `$82`/`$83` (mute/unmute) and `$C0..$CF` attenuation through the two AD7528
  devices, determine whether the final analog boundary attenuates both CDIC and
  DVC audio, and add state-transition/mix tests before changing audible output.
  A Hotel Mario score-screen SPI capture now supplies a timing oracle: 55
  four-byte-equal DAC writes, ordinary spacing about 0.775-0.825 ms, a
  0-to-255 rise over 27.132 ms, and a 252-to-0 fall over 24.031 ms, with
  workload stalls up to 2.9 ms. Still needed are simultaneous ch2 command,
  analog-output, and CDIC/DVC-source traces to establish polarity, gain curve,
  mute behavior, and the shared output boundary.
- [ ] Add a concise `docs/vmpeg-architecture.md` for outside reviewers. Diagram
  the guest/CDIC/main-RAM/DMA2/FIFO/PES/decoder/MCD251/MCD212/audio path and give
  a component-by-component provenance table: Philips specifications and
  firmware evidence, clean-room MiSTer behavioral checks, the attributed
  MIT `gen2brain/mpeg` video translation, and `oxideav-mp2` audio. Explain that
  structural similarity to MiSTer's MPEG decoder is expected because both
  descend from `pl_mpeg`, while E-Di's VMPEG device integration is independent.

## Documentation research

- [ ] Search the indexed corpus systematically for MCD212 display/timing,
  CDIC audio/transport, SLAVE/SERVO, DVC/VMPEG, input, storage, and player-model
  behavior. The first priority pass is recorded in
  `docs/specification-research.md`; continue through the OCR-completed service
  manuals and lower-priority technical notes.
- [ ] Record useful findings with exact edition/page provenance and add a
  focused device test before changing emulation. The initial compliance matrix
  and ordered test queue are in `docs/specification-research.md`; the tests are
  intentionally still pending.

## Research-derived device work

These are ordered by compatibility impact and dependency. A checked item
requires the cited device-level test as well as the implementation change;
none should be implemented as a title-specific workaround.

- [ ] Add one diagnostic timeline containing the independent 75-sector disc
  clock, 50/60-field clock, 10 ms system tick, audio-buffer events, PCL
  ownership, and pointer updates.
- [ ] Model PAL's 30.0000 MHz and NTSC's 30.2098 MHz board clocks only as part
  of cycle/event-interleaved scheduling. Add line/field and CPU/device cadence
  tests first; do not replace the global clock constant in isolation.
- [ ] Resolve the CDIC register-2 reset state at base + `$3FFA`. Four service
  manuals consistently document `$C7FE` in the PCB test and `$D7FE` in the
  terminal test, while the emulator starts at zero. Trace the BIOS first
  accesses and identify the test-path bit-12 difference before choosing a
  reset value.
- [x] Correlate the SERVO 2.1 firmware SPI framing and implemented
  `A0..A5`/`AB` paths with the service-manual four-byte contract. The
  generated disassembly, vector map, buffers, scheduler, and revision
  discrepancies are recorded in `docs/slave-protocol.md`.
- [ ] Trace the retained-disc bit from the SERVO tray/medium/spin-up state,
  through the SLAVE SCI receive path, to channel-3 `B0` byte 2 bit 6. Add
  read-only trace points for the raw packet, derived flags, and resulting
  host packet before changing HLE behavior.
- [ ] Verify VMPEG guest-visible CSD and memory descriptors: `/mv`, `/ma`,
  `RAM00`, `RAM01`, priority `$81`, color `$90`, and the guaranteed 960 KiB
  contiguous extension allocation.
- [ ] Add project-owned MPEG transition fixtures for display-timed first/last
  picture events, EOS+SOS recovery, delayed B-pictures, pause/continue,
  abort/restart, redundant EOS, input errors, and old-PCL release.
- [ ] Implement the documented keyboard K-mode serial boundary first: device
  recognition, 1200-baud two-byte state-change packets, modifiers,
  press/release, and idle silence. Add T-mode only afterward.
- [ ] Before changing the current `CD-XA001` gate, resolve its full CD-ROM XA
  label-record requirement separately from BRIDGE10's `$20`/System-Identifier
  startup discriminators. Add session `PSEC`, global-LSN, repeated-SVD, and
  last-session remapping only when the disc model can represent multisession
  TOCs.
- [ ] Keep CD-i 450/550 DVC variants (`22ER9144`/`22ER9956`) outside the
  22ER9141 M3 implementation until their separate boards, firmware, and memory
  interfaces are inventoried for a later player model.

## Specification-driven diagnostics

- [ ] Add read-only runtime OS-9/CD-RTOS awareness to diagnostics without
  replacing or bypassing guest execution. Implement it incrementally:
  validate the 68000 vector-table longword at address zero as the System
  Globals pointer for each supported ROM, then resolve the module directory
  and current-process descriptor. Add active/sleep/wait queue and scheduler
  state only after those roots and offsets pass ROM-specific validation.
  Observe `TRAP #0` system calls without replacing them; then identify
  processes, path/device descriptors, driver calls, wait reasons, and relevant
  kernel structures. Expose the results through bounded snapshots and incident
  timelines so a stall can be reported as a guest process waiting on `/cd`,
  `/mv`, `/ma`, CDFM, or another service rather than only as a raw PC address.
  Fail closed to raw machine diagnostics whenever a pointer, module header, or
  layout check fails. TN 072 makes the first path-level fixture concrete:
  capture one `os9forkc()` or `chainc()` handoff, verify that paths `0..N-1`
  are inherited, and classify the child's actual `SS_Opt` buffers as CDFM
  CD/audio or UCM keyboard/pointer/video. Its archived PDF omits printed page
  2, so use the cross-checked Green Book chapter-VII option layouts and
  ROM-specific kernel evidence before decoding process-local path-table
  structures.
  - [x] Establish the fail-closed foundation: snapshots now report static ROM
    identity/module count, the mapped/aligned state of address zero, and at
    most 64 independently CRC-valid modules found in live RAM. The CD-i 220 F2
    BIOS reaches a mapped-RAM candidate at `$1500`; this is not yet treated as
    a validated System Globals layout.
  - [x] Implement TN 072's exact, packet-relative `SS_Opt` classifier for CDFM
    CD/audio and UCM keyboard/pointer/video, plus the contiguous inherited-path
    range `0..N-1`. Runtime discovery of the child's actual path table and
    option buffers remains open.
  - [ ] Validate ROM-specific System Globals, module-directory, current-process,
    scheduler-queue, and path-table offsets before exposing those structures.
- [ ] Extend the implemented PCB/CIL/PCL ownership and DMA hash trace with
  bounded guest-write provenance and UCM drawmap operations so diagnostics can
  hash every stage from selected sector through MCD212 plane.
- [ ] Add contextual RL3/RL7/CLUT/DYUV previews and optional local XA
  WAV/MPEG-frame previews.
- [ ] Add project-owned fixtures that damage individual provenance stages and
  prove `diagnose compare` locates the first divergence.
- [ ] Complete current-build local pilots. The known-good sustained-MPEG
  comparator (The 7th Guest) and current VCD transport run are recorded; a
  known-good base boot and base RTF display sequence remain.

## Disc loading and player state

- [ ] Support Redump-style CD+G disc sets, including the store-only `.zip`
  archives under `Disc Images/CD+G`. Extend the existing safe Store-ZIP path
  to retain and resolve multi-`.bin` + `.cue` + `.cdg` layouts, inventory the
  CD+G subchannel stream, and pass the reconstructed disc through the CD-i
  player's native CD+G support rather than adding a host-side playback bypass.
  Use `whatever-industries/cdg-player` as the format/parser reference and add
  fixtures for archive eligibility, relative-path preservation, cue/track
  resolution, CD+G packet timing, audio synchronization, and native player
  launch. Continue rejecting compressed ZIP entries initially.
- [ ] Decide whether a fully live same-standard disc swap can be restored only
  after the SLAVE/CD-RTOS removal and insertion sequence is understood and
  covered by device tests.

## Saved games and timekeeper

- [ ] Choose a frontend timekeeper policy. The deterministic core date is
  useful for tests but can make timestamped saves indistinguishable; host time
  needs calendar conversion and an explicit timezone policy.

## Input and frontend

- [ ] Continue the behavior-neutral refactoring sequence in
  `docs/code-audit.md`. Keep structural checkpoints separate from emulation
  corrections and verify each extracted responsibility independently.
- [ ] Implement the optional `/pck` player-control-key peripheral documented
  by Philips TN 73.2. Advertise it through the CSD, cover `KB_Read`,
  `KB_Rdy`, `KB_SSig`, and `ss_enable`, then offer an explicit choice between
  routing Start to native Pause `$82` key-down/key-up events or retaining its
  current host-menu role. Until then Start defaults to the configurable
  host-level E-Di menu; Guide/Home, L1+R1, and right-stick alternatives are
  available, and Select is unassigned. Start must never become a third base
  pointer button.
- [ ] Emulate the CD-i keyboard peripheral and expose host-keyboard passthrough
  behind a Settings toggle after the K-mode serial/device tests above pass.
- [ ] Complete controller navigation and accessibility passes for every
  Library and Settings control.
- [ ] Keep Settings panels compact at the default window size without
  unnecessary scrolling or descriptive text.
- [ ] Add an optional, model-specific hardware-player front-panel display.
  Drive track/time/status indicators from emulated SLAVE/player state, keep it
  disabled by default, and verify the chosen CD-i 200/210/220 panel against
  Philips service documentation. MAME's front-panel support may be used as a
  behavioral/reference comparison, but do not infer panel state from title
  pixels or frontend playback state.

## Compatibility and automated coverage

- [ ] Model the documented regional board clocks and add a synthetic
  within-instruction device-deadline test before deciding whether finer event
  slicing is required. Revalidate the CD Shoot
  language-flag/skeet cadence while preserving The 7th Guest transitions,
  Earth Command audio termination, The Naked Gun copyright transition, and
  Alien Gate input plus its real-hardware-matched firing-audio cutoff after the
  bat-enemy sequence. See
  `data/compatibility/incidents/cd-shoot-fast-hover-animation.json`.
- [ ] Compare The 7th Guest's three brief transition intervals with the same
  sequence on hardware before deciding whether any blank fields are missing
  imagery or normal authored/loading behavior. See
  `data/compatibility/incidents/7th-guest-missing-transition-animations.json`.
- [ ] Locate one or two brief audio hits near the start of The 7th Guest's
  opening Philips clip by comparing source MP2, decoded PCM, mixed core PCM,
  and frontend output. See
  `data/compatibility/incidents/7th-guest-philips-logo-audio-hits.json`.
- [ ] Isolate The 7th Guest's occasional brief music stutter by first
  identifying the active audio source, then correlating CDIC/VMPEG buffer
  events, pre-mix PCM, mixed PCM, and CPAL callback occupancy. See
  `data/compatibility/incidents/7th-guest-intermittent-music-stutter.json`.
- [ ] Test the Engineering 6.0 CD-i disc image as an optional local smoke and
  compatibility case. Record its exact disc fingerprint, player standard,
  boot/menu checkpoint, input, audio, and representative display behavior
  without committing the image or its path.
- [ ] Turn the two Philips Validation Discs into an optional, local regression
  matrix. Start with specification-defined Disc 1 bitmap/UCM, pointer, NVRAM,
  alarm/signal, and real-time-video cases; then cover Disc 2 DYUV, RL7, XA,
  channel-selection, trigger, and EOR/EOF records. Treat performance numbers
  as observational until the exact player revision and hardware baseline are
  known. See
  `data/compatibility/incidents/philips-validation-disc-coverage.json`.
- [ ] Follow automated coverage with focused manual passes for input, timing,
  audio synchronization, display composition, and gameplay transitions.

## Display geometry and disc profiles

The Green Book defines 525/60 versus 625/50 as a player property. The Disc
Label has no standardized PAL/NTSC requirement field, so region names and
asset dimensions are supporting evidence rather than ground truth.

- [ ] Record live MCD212 mode, scan/compatibility state, DVC sequence format,
  and evidence distinguishing authored bars from emulator margins.
- [ ] Make filename-region matching explicitly a compatibility guess and
  eventually disable it by default once exact-disc coverage is sufficient.
- [ ] Let dual-standard headless runs propose candidate profiles; require
  manual confirmation before tracking uncertain results.
- [ ] Revalidate only display discrepancies still visible on the current
  build. Historical title reports are collected in
  `data/compatibility/incidents/legacy-display-framing-concerns.json`; do not
  restore title-name crops or content-based border detection.
- [ ] Determine whether the NTSC player menu is vertically over-scaled relative
  to the PAL player menu or whether the difference is authored BIOS layout.
  See `data/compatibility/incidents/player-menu-pal-ntsc-sizing.json`.
- [ ] Determine whether The 7th Guest's NTSC/PAL title-screen framing difference
  comes from the two authored MPEG streams, VMPEG display-window programming,
  or host presentation. See
  `data/compatibility/incidents/7th-guest-pal-ntsc-title-sizing.json`.
- [ ] Find the first display-provenance stage where Alien Gate loses the lower
  edge of its elevated HUD. Preserve the hardware-confirmed windowboxed
  playfield, bottom black bar, complete HUD, and projectile spill; do not
  assume a plane-priority fault until source, plane, raster, and frontend
  captures distinguish it. See
  `data/compatibility/incidents/alien-gate-hud-lower-edge-missing.json`.
- [ ] Compare equivalent Alien Gate USA Rev 1 and Europe Rev 2 gameplay frames
  through live MCD212 state and display provenance. Europe is four-sided
  windowboxed in the available real-hardware reference; USA currently has no
  bottom bar. Do not normalize the two releases without hardware evidence.
  See
  `data/compatibility/incidents/alien-gate-pal-ntsc-framing-difference.json`.

## MPEG/DVC and Video CD

- [ ] Compare Philips FMVDemo backward scan with a real VMPEG cartridge and
  trace GOP entry, reference state, slice coverage, and presentation before
  deciding whether its transient black macroblocks are authentic. See
  `data/compatibility/incidents/fmvdemo-rewind-macroblocking.json`.
- [ ] Diagnose the native Video CD `Contents` action becoming unresponsive
  while MPEG playback continues. Capture SLAVE input, guest PC/IRQ, CDIC/PSD
  commands and completions, and VMPEG counters around the selection. See
  `data/compatibility/incidents/vcd-contents-control-freeze.json`.
- [ ] Script real feature playback with `--click-event`, capture
  `--dump-vmpeg-es`, and compare it with the contiguous on-disc stream.
- [ ] Complete M3.14 transition and long-run coverage. Project-owned TN 088
  core regressions now pass for EOS+SOS, pause/continue, abort/restart
  stale-picture removal, delayed last-picture display, PES stream switching,
  128 repeated transitions, and a six-hour integer A/V clock mapping. The
  local runner now emits payload-free per-play timing/counter summaries and
  raster-sequence hashes. Two exact 1.1-billion-instruction runs produced
  byte-identical five-play diagnostics/summaries and zero decoder errors; an
  optional local baseline now makes those stable fields executable. Next,
  title-level manual pause/continue and stream-switch gates now exist using
  Philips FMVDemo. The automated native pause/continue gate now passes after
  correcting DBUF/stop transport semantics and programming VMPEG's separate
  PAL/NTSC display-period register; manual GUI confirmation also passes. The
  multilingual switch now waits for the first selected PES boundary, retains
  complete old-stream frames, discards only the incomplete tail, and conceals
  one missing partial Layer-II frame. Its exact native gate continues with
  zero decoder errors or underflows. Next, cover the cake puzzle and devise a
  timestamp/presentation-overlap drift oracle. The first startup-provenance
  slice is implemented: a DMA batch no longer assigns its final PES PTS to the
  first queued decoded output. The Addams USA trace removes 408 ms of false
  phase offset with zero decoder errors. Addams, Naked Gun, Pete Townshend,
  and Philips FMVDemo now pass manual synchronization checks. If long-run
  drift appears, propagate PTS to later decoded access units rather than
  adding a fixed delay. Keep
  `docs/mpeg-dvc-plan.md` current.
- [ ] Populate a low-profile VCD panel beneath the display with play, pause,
  stop, seek/track, status, and eject controls while retaining CD-i menu input.
- [ ] Add native-control tests and local media-gated A/V synchronization tests.

## Photo CD

- [ ] Capture the remaining native Photo CD moving-transition boundary over a
  longer consecutive-field burst. Stable Photo CD output and Dark Chaser's
  magenta final column were corrected by retaining the actual field parity;
  the only current report is field structure confined to the moving wipe.
  Compare guest plane-RAM changes, decoded planes, composed raster, and live
  registers with `scripts/summarize-display-fields.py`; do not add a filter or
  deinterlacer. See
  `data/compatibility/incidents/native-photocd-transition-boundary-interlace.json`.
- [ ] Record the native Photo CD application's live standard, display mode,
  origins, magnification, and aperture when horizontal bars are visible. Photo
  CD v0.9 defines a 768×512 square-pixel (3:2) Base image and its informative
  appendix shows different NTSC/PAL 0/5/10% overscan mappings; do not turn the
  bars into a crop heuristic. Compare similarly sized bars in European CD-i
  reports only after proving identical MCD212 state.
- [ ] Consider an explicit optional `TV 4:3` preview beside the existing
  source-pixel Photo CD view; keep `View Raw Images` uncropped and clearly
  identify the two presentation semantics.
- [ ] Add synthetic detection/control tests and local image-quality,
  orientation, and navigation passes.

## Repository policy

- [ ] Keep `unsafe_code = "deny"` and zero-warning clippy.
- [ ] Never commit commercial discs, extracted title media, private reference
  downloads, ROMs outside the explicitly supported `firmware/` policy, or
  local diagnostic captures.
- [ ] Commit to `main`; push only manually accepted stable checkpoints, then
  produce optimized builds from a clean local commit.
