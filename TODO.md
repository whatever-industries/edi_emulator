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
- [x] Record `reported_revision`, `last_reproduced_revision`,
  `last_verified_revision`, and an evidence status for compatibility incidents.
  Unknown historical revisions stay explicitly unknown rather than being
  guessed.

## External technical-review follow-ups (2026-07-31)

- [ ] Clarify the player-generation roadmap before adding another board.
  E-Di's current CD-i 200/210/220 implementation is already **Mono-I**. Inventory
  the device maps, interrupt wiring, SLAVE/IKAT behavior, storage, firmware, and
  DVC interfaces of Mono-II, Mono-III/Roboco, and Mono-IV, then select the next
  model explicitly. Keep the current M4 boundary of CD-i 490/Mono-IV + IMPEG
  unless evidence justifies prioritizing a different generation.
- [ ] Revalidate the public-build "video works but all audio is absent" report
  on the reporter's Debian output device after PR #1 / merge `36b46f7`.
  The frontend now prefers I16/F32 over PulseAudio's potentially first-listed
  U8 format and has a tested U8 fallback; a device exposing only 48 kHz can
  still disable audio independently of CDIC, SLAVE, or VMPEG behavior.
- [ ] Finish portable frontend audio after that retest: add deterministic
  44.1 kHz-to-host-rate conversion where necessary, preserve channel mapping,
  and keep an audio-start failure persistently visible instead of allowing
  silent video playback.
- [x] Add SCC68070 bus-fault signaling, the documented 17-word vector-2 frame,
  RR-aware long-frame `RTE`, and focused mapped/unmapped access tests. The two
  externally verified Mono-I absent-memory ranges now fault; unverified holes
  deliberately retain open-bus behavior. At the same 30-million-instruction
  checkpoint, the corrected build reaches the player shell while `3bfb46d`
  remains on cyan, reducing cycles from 330,734,670 to 159,555,961.
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
  macOS now reproduces the lower-half flicker, and the user confirmed it is
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
- [x] Verify CDIC audio-buffer ordering and AUDCTL start semantics. Mono-I
  hardware captures in `CDIC_BlackBoxAnalyzer` revision `e861f76` show the
  first CD-fed XA sector in `$2800`/DBUF 4, the next in `$3200`/DBUF 5, and
  require guest `AUDCTL $0800` before CDDA or XA playback. Device tests now
  protect buffering-before-playback and the one-shot `$ff` stop latch. This is
  separate from VMPEG Layer-II audio and from the still-open SLAVE analog mute
  boundary.
- [ ] Add a concise `docs/vmpeg-architecture.md` for outside reviewers. Diagram
  the guest/CDIC/main-RAM/DMA2/FIFO/PES/decoder/MCD251/MCD212/audio path and give
  a component-by-component provenance table: Philips specifications and
  firmware evidence, clean-room MiSTer behavioral checks, the attributed
  MIT `gen2brain/mpeg` video translation, and `oxideav-mp2` audio. Explain that
  structural similarity to MiSTer's MPEG decoder is expected because both
  descend from `pl_mpeg`, while E-Di's VMPEG device integration is independent.

## Documentation research

- [x] Extract or OCR all 185 PDFs in the local Philips CD-i/ICDIA reference
  archive with `scripts/ocr-cdi-specs.sh`. The rebuilt manifest has one unique
  source and SHA-256 row per PDF with no missing text sidecars: 141 used
  embedded text, 23 required 240 dpi Tesseract OCR, and 21 reused previously
  validated local sidecars. Generated text remains under ignored reference
  storage. TN 076's archived scan still omits printed page 3, so recovering a
  replacement source remains open even though this edition is searchable.
- [ ] Search the indexed corpus systematically for MCD212 display/timing,
  CDIC audio/transport, SLAVE/SERVO, DVC/VMPEG, input, storage, and player-model
  behavior. The first priority pass is recorded in
  `docs/specification-research.md`; continue through the OCR-completed service
  manuals and lower-priority technical notes.
- [ ] Record useful findings with exact edition/page provenance and add a
  focused device test before changing emulation. The initial compliance matrix
  and ordered test queue are in `docs/specification-research.md`; the tests are
  intentionally still pending.
- [x] Review and visually verify the cross-subsystem TN 022/034/068/086 batch.
  The ledger now distinguishes planar RGB555 IFF storage from interleaved UCM
  buffers, records DYUV modulo/per-line-reset behavior, and separates native
  NVRUI policy from the physical timekeeper/SRAM device. No behavior change was
  warranted; focused RGB555/DYUV fixtures now pass, while the model-specific
  NVRAM fixture remains queued.
- [x] Review and visually verify TN 039/048/099/100. TN 039's analog composite
  coloration remains an optional presentation concern; TN 048/099 corroborate
  authored 525/625 centering and asynchronous sector-buffer routing; TN 100
  records display-fetch-timed interrupts and the possible half-written active
  LCT race. The follow-up matte fixture found and corrected shared-register
  priority: simultaneous path-A/path-B loads now retain path A. A bounded
  DCA/LCT fetch timeline remains queued before any scheduling change.
- [x] Review and visually verify TN 053. UVLO is a title-supplied base-case
  software codec that receives frames through circular PCLs, decodes into
  alternating DYUV drawmaps, and exposes the window with MCD212 mattes; it is
  not a VMPEG format or new MCD212 coding mode. TN 089/090/094/104 were already
  indexed in the research ledger and compliance matrix. A redistributable UVLO
  fixture, or an optional Engineering 6.0 local case, remains queued.
- [x] Study and index `CD-I full-motion video encoding on a parallel
  computer.pdf` from the local Philips CD-i reference folder. The research
  ledger now records its early 1.2 Mbit/s encoder assumptions, reconstructed
  reference-frame loop, decoder timing/reordering duties, 50/60 Hz conversion,
  and pixel-by-pixel base-video composition. Treat it as architectural
  corroboration rather than a replacement for the final Green Book/TN 097.

## Research-derived device work

These are ordered by compatibility impact and dependency. A checked item
requires the cited device-level test as well as the implementation change;
none should be implemented as a title-specific workaround.

- [x] Decode guest PCB/CIL/PCL state at DMA boundaries and add a synthetic
  circular-buffer test that detects producer reuse before `/mv` or `/ma`
  releases a full PCL. A current comparison found orderly one-sector
  fill/release cycles and no overwrite risk in The 7th Guest or the sampled
  Addams Family Values USA VCD run.
- [x] Extend that read-only trace with full PCB state. Diagnostics now report
  the EOR-delimited `PCB_Rec` count, the transition to zero, re-arming,
  `PCB_Chan`, `PCB_AChan`, and all three CIL pointers. The Philips *master Disc
  Building Utility* example confirms the record-count interpretation and the
  distinction between channel selection and direct-audio routing.
- [x] Add a native CDFM play-termination fixture contrasting normal
  EOR/`PCB_Rec` exhaustion with `ss_abort` during direct audio. Philips FPD805
  `bmp_nat` now proves both paths return to the player. Its abort handler clears
  the channel-15 route during fade-down before it later calls `ss_abort`.
- [x] Add a fixture that writes `PCB_Rec` to zero mid-play during direct audio,
  including Philips' `PCB_AChan` workaround with both zero and nonzero audio
  CIL entries. Exact disc/ROM preflight and logged before/after field hashes
  guard its fixed native addresses. All three current 220 cases recognize the
  clear in the next selected audio-sector handler; no CDFM behavior was
  duplicated inside CDIC.
- [x] Isolate the VCD audio divergence at `$22CD88`. Guest-write provenance
  proved the application intentionally changes two SCR timestamp bytes before
  DMA2; the MPEG audio payload is unchanged. The legal 647-byte mid-frame
  prefix is now synchronization distance rather than 647 malformed-frame
  errors. Two deterministic repeats end with zero demux/video/audio/stream
  errors. See `docs/mpeg-dvc-plan.md`.
- [ ] Add one diagnostic timeline containing the independent 75-sector disc
  clock, 50/60-field clock, 10 ms system tick, audio-buffer events, PCL
  ownership, and pointer updates.
- [ ] Model PAL's 30.0000 MHz and NTSC's 30.2098 MHz board clocks only as part
  of cycle/event-interleaved scheduling. Add line/field and CPU/device cadence
  tests first; do not replace the global clock constant in isolation.
- [x] Add PAL and NTSC register-level tests for the MCD212 cursor blink and
  correct it to exactly 12 television fields per programmed on/off unit:
  240 ms at 50 Hz and 200 ms at 60 Hz.
- [x] Add TN 022/034/086 renderer regressions for RGB555 plane-A/high plus
  plane-B/low assembly, modulo-256 DYUV addition, and fresh programmed Y/U/V
  starts on every scanline. The existing implementation passes without a
  behavior change; authored IFF conversion, panning, and fitted blits remain
  outside the MCD212 device.
- [x] Add Green Book/TN 099 matte regressions for false scanline starts,
  persistent register commands, ordered one-/two-set operation, STOP, and
  simultaneous shared-register loads. The fixture exposed path B overwriting
  path A; paired ICA/DCA phases now enforce the specified path-A priority while
  preserving non-conflicting path-B writes.
- [x] Decode Green Book `cp_dprm` BP/PRF/RMS fields as emitted by Philips
  UCM software. BP_DOUBLE now selects MCD212 `DCR.CM` through both ICA and DCA;
  the prior MAME-style low-five-bit mapping dropped the captured high-resolution
  request. See
  `data/compatibility/incidents/mcd212-display-parameter-bp-mapping.json`.
- [ ] Resolve the CDIC register-2 reset state at base + `$3FFA`. Four service
  manuals consistently document `$C7FE` in the PCB test and `$D7FE` in the
  terminal test, while the emulator starts at zero. Trace the BIOS first
  accesses and identify the test-path bit-12 difference before choosing a
  reset value.
- [ ] Correlate SERVO firmware SPI traffic with the documented four-byte
  `A0..A5`/`AB` command path, then cover drive open, close, spin-up, status,
  time, track/index, and live-media transitions with SLAVE tests.
- [ ] Verify VMPEG guest-visible CSD and memory descriptors: `/mv`, `/ma`,
  `RAM00`, `RAM01`, priority `$81`, color `$90`, and the guaranteed 960 KiB
  contiguous extension allocation.
- [ ] Add project-owned MPEG transition fixtures for display-timed first/last
  picture events, EOS+SOS recovery, delayed B-pictures, pause/continue,
  abort/restart, redundant EOS, input errors, and old-PCL release.
- [x] Test CDIC sound-map transfer completion separately from audible
  completion, including one- and two-sector maps, stop/replacement, and the
  buffered audio tail.
- [ ] Implement the documented keyboard K-mode serial boundary first: device
  recognition, 1200-baud two-byte state-change packets, modifiers,
  press/release, and idle silence. Add T-mode only afterward.
- [x] Extend the synthetic Video CD Bridge/PVD fixture through `INFO.VCD`,
  `ENTRIES.VCD`, `LOT.VCD`, and `PSD.VCD`. The payload-free inventory records
  White Book entry points and selection/play/end topology and was validated on
  a local Video CD 2.0 disc. Correlate the native dirty-disc dialog with the
  first transport or decoder failure rather than treating it as proof of bad
  media.
- [x] Add payload-free Photo CD Bridge classification and a synthetic PVD/CD-i
  application fixture while preserving the disc's native application beside
  host viewer controls. The same classifier was validated against a local
  compliant Photo CD without retaining media payload.
- [ ] Keep CD-i 450/550 DVC variants (`22ER9144`/`22ER9956`) outside the
  22ER9141 M3 implementation until their separate boards, firmware, and memory
  interfaces are inventoried for a later player model.

## Specification-driven diagnostics

- [x] Add incident lifecycle commands, bounded snapshots/events, deterministic
  comparison, context-aware experiment outcomes, sanitized promotion,
  verification checklists, and compatibility reports. See
  `docs/debugging-workflow.md`.
- [x] Add payload-free `DiscInventory` inspection for CUE identity, TOC,
  Green Book and ISO/VCD filesystems, OS-9 modules, RTF sector classes, and
  MPEG sequence headers.
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

- [x] Insert the first disc into an already-running empty player through the
  SLAVE media-change path.
- [x] Cold-power-cycle when replacing mounted media. This deliberately favors
  reliable title replacement over retaining the previous guest application's
  volatile state, while preserving NVRAM, timekeeper state, firmware, and the
  newly selected disc.
- [x] Verify CD-i PAL-to-PAL replacement on `ac20dcd`; the replacement boots
  after selecting Play from the freshly booted player shell. See
  `data/compatibility/incidents/live-disc-replacement.json`.
- [x] Verify first insertion and replacement for VCD and Photo CD, including
  ZIP-backed images and replacement without an intervening eject.
- [ ] Decide whether a fully live same-standard disc swap can be restored only
  after the SLAVE/CD-RTOS removal and insertion sequence is understood and
  covered by device tests.

## Saved games and timekeeper

- [x] Persist one atomic `<board>.nvr` file per board and preserve it across
  resets, media replacement, and same-board ROM changes.
- [x] Back up player storage before a user-requested clear.
- [x] Invalidate only generated CD-RTOS `csd` data when player configuration
  changes, preserving title saves.
- [x] Confirm on `ac20dcd` that a real title save survives application quit
  and relaunch.
- [x] Verify that the player MEMORY screen reports plausible free space. On
  `019fb59`, three retained entries report 13.3%, 2.4%, and 2.0%; their rounded
  17.7% sum agrees with the shell's 18% total, leaving approximately 82% free.
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
- [x] Display known parental passcodes in a muted amber player notice: Voyeur
  `3333`, Vegas Girls `1234`, and Loving for a Lifetime `6969`.
- [ ] Identify and add the verified passcode for
  `Pleasures of Sex, The (Europe)`.

## Compatibility and automated coverage

- [x] Enable Philips section-6.2 SCC68070 instruction/bus timing and reconcile
  the resulting The 7th Guest transport divergence with the documented CD-i
  220 shared-IN4 CDIC/VMPEG daisy chain. The four timing-table tests and
  bounded zero-decoder-error title trace now pass.
- [ ] Model the documented regional board clocks and add a synthetic
  within-instruction device-deadline test before deciding whether finer event
  slicing is required. Revalidate the CD Shoot
  language-flag/skeet cadence while preserving The 7th Guest transitions,
  Earth Command audio termination, The Naked Gun copyright transition, and
  Alien Gate input plus its real-hardware-matched firing-audio cutoff after the
  bat-enemy sequence. See
  `data/compatibility/incidents/cd-shoot-fast-hover-animation.json`.
- [x] Revalidate The 7th Guest's full clean-NVRAM opening under accurate
  SCC68070 timing and shared-IN4 arbitration. The pre-title clip, title MPEG,
  both post-title MPEG stages, visible stairwell transition, automatic
  gameplay entry, and gameplay audio proceed.
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
- [x] Build an automated headless pass that exercises titles, captures bounded
  screenshots/audio/diagnostics, detects stalls or panics, and records results
  before a finer manual pass. Local suite manifests use explicit assertions
  and wall-clock/instruction bounds; accepted results promote without paths to
  `data/compatibility/title-matrix.json`.
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
- [x] Capture consecutive source fields, decoded planes, post-weave raster,
  and frontend output for Validation Disc 1's `Visual Examples > Drawing >
  CLUT 4 High Res` scene. Four stable fields and a raw-nibble comparison prove
  the one-pixel contours are authored in the field-separated drawmaps and
  faithfully preserved, not introduced by UCM, weave, or presentation. See
  `data/compatibility/incidents/validation-disc-clut4-highres-field-artifacts.json`.
- [x] Maintain a title matrix with exact disc fingerprint, model, standard,
  DVC requirement, boot/gameplay state, issues, and last-tested revision.
  It starts empty and is populated only by manually accepted local suite
  checkpoints.
- [ ] Follow automated coverage with focused manual passes for input, timing,
  audio synchronization, display composition, and gameplay transitions.
- [x] Accept store-only, unencrypted, one-CUE ZIP images in the Library,
  File → Open, and `cdi-frontend --disc`.

## Display geometry and disc profiles

The Green Book defines 525/60 versus 625/50 as a player property. The Disc
Label has no standardized PAL/NTSC requirement field, so region names and
asset dimensions are supporting evidence rather than ground truth.

- [x] Make `cdi-core`'s MCD212 `DisplayGeometry` authoritative for rendering,
  screenshots, window aspect, and pointer mapping. Remove the host-created
  analog-overscan crop and always present the hardware aperture. See
  `docs/display-geometry.md`.
- [x] Add exact ordered-CUE SHA-1 profiles and fetch the Redump CD-i DAT from
  `redump.info` into ignored reference storage.
- [x] Remember an explicit PAL/NTSC override by exact pressing fingerprint and
  retain a clear `Use detected default` action.
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
- [x] Enable native White Book type-4 insertion and the VMPEG cartridge's
  `$E01000` output-clock converter. Hardware review confirms that the 13.5 MHz
  change is performed by a separate circuit downstream of MCD251, not an
  MCD251 phase register, and expands horizontal output by `15/13.5` (`10/9`).
  The code now owns this conversion at that cartridge boundary. Keep source
  crop/window coordinates distinct and investigate remaining Addams UK
  positioning only from guest register state plus synchronized hardware
  output. See
  `data/compatibility/incidents/addams-usa-right-edge-black-column.json`.

## MPEG/DVC and Video CD

- [x] Diagnose transient horizontal tearing across VMPEG pictures. First
  distinguish mid-raster picture publication, mismatched retained fields, and
  a frontend frame-copy race by tagging scanlines/fields with immutable frame
  generations. See
  `data/compatibility/incidents/vmpeg-horizontal-frame-tearing.json`. A
  specification-backed candidate now stages decoded pictures and latches the
  MCD251 video-generator source only at VSYNC. The native FMVDemo
  pause/continue gate presents 302 pictures with zero decoder errors and zero
  mixed-generation fields. Manual A/B testing confirms the latch removes the
  original moving-scene slice and the exact pre-fix base restores it. The A/V
  offset present in both builds is tracked separately.
- [ ] Compare Philips FMVDemo backward scan with a real VMPEG cartridge and
  trace GOP entry, reference state, slice coverage, and presentation before
  deciding whether its transient black macroblocks are authentic. See
  `data/compatibility/incidents/fmvdemo-rewind-macroblocking.json`.
- [ ] Diagnose the native Video CD `Contents` action becoming unresponsive
  while MPEG playback continues. Capture SLAVE input, guest PC/IRQ, CDIC/PSD
  commands and completions, and VMPEG counters around the selection. See
  `data/compatibility/incidents/vcd-contents-control-freeze.json`.
- [x] Compare current PCB/CIL/PCL ownership and CDIC-to-DVC payload hashes
  against a working sustained-MPEG title. No full-buffer reuse was observed;
  The 7th Guest remained error-free, while Addams had one audio divergence.
- [ ] Script real feature playback with `--click-event`, capture
  `--dump-vmpeg-es`, and compare it with the contiguous on-disc stream.
- [x] Revalidate the five rare B-picture failures recorded by the original M3
  long run. Two exact current five-play runs are deterministic and error-free;
  the historical failures predated the shared-IN4 transport correction and
  are retained as superseded evidence rather than hidden by decoder recovery.
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
- [x] Detect VCD media and its root `CDI` application without bypassing the
  disc's own menu.
- [ ] Populate a low-profile VCD panel beneath the display with play, pause,
  stop, seek/track, status, and eject controls while retaining CD-i menu input.
- [x] Add synthetic VCD Bridge detection and White Book PSD topology tests.
- [ ] Add native-control tests and local media-gated A/V synchronization tests.

## Photo CD

- [x] Detect Photo CD images by filesystem/content evidence, including the
  root `CDI` directory. `DiscInventory` now records the Bridge signature,
  native application presence, and specific `photo-cd` classification; the
  host viewer retains its independent image-pack validation.
- [x] Populate Photo CD navigation and image controls in the existing bottom
  player bar while preserving the disc's CD-i application behavior.
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
- [x] Confirm that View Raw Images preserves the complete 3:2 source while the
  native application/hardware reference uses the expected centered 4:3 soft-
  display crop. The raw viewer's symmetric bars are not a geometry bug.
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
