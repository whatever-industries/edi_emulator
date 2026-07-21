# E-Di: Emulator Disc Interactive TODO

## Disc insertion without a machine reset

- [ ] Load a disc the way real hardware does, instead of cold-booting. The
  emulator starts booting as soon as the app launches, but `LoadDisc` in the
  frontend's emu thread calls `machine.reset()` so the BIOS sees the disc at
  power-on, which throws that boot away and makes the user sit through a
  second ~25-second boot after picking a disc from the library.
  - [ ] Signal media change through the SLAVE drive status so the already
    running player shell detects the insertion and launches the disc's
    application, rather than restarting from the reset vector.
  - [ ] Keep an explicit reset path for the cases that need it (eject, DVC
    insert/remove, system ROM change) and for discs that misbehave.
  - [ ] Verify against CD-i, VCD, and Photo CD discs that the shell picks up
    the disc reliably, including swapping one disc for another without an
    intervening eject.

## Saved games (NVRAM persistence)

- [x] Persist NVRAM so saved games and player settings survive a restart.
  The frontend now restores the battery-backed SRAM at startup and writes it
  back whenever the contents change (checked every five seconds, plus a final
  flush on exit), one `<model>.nvr` file per model in the platform data
  directory. Verified both directions: a seeded buffer is restored, and the
  BIOS reinitialising it is written back.
  - [x] The timekeeper turned out not to conflict. The MK48T08 clock
    registers (`$1FF8`-`$1FFF`) are separate struct fields rather than part
    of the saved buffer, so a stale clock cannot be restored over a fresh one.
  - [x] The NVRAM window is not a discrepancy: 8 KB is the real MK48T08 size,
    and the larger `nvram_size` window simply mirrors it.
  - [ ] Decide what the clock should read. It is seeded to a fixed date
    (1995-06-13 12:00) to keep the core deterministic for hashes and tests.
    A title that timestamps saves will stamp every one identically, and "most
    recent" ordering may misbehave. Host time in the frontend while the CLI
    keeps the deterministic seed would fix it, but needs calendar conversion
    and a timezone source.
  - [ ] Test with titles that actually save, confirming a save survives quit
    and relaunch, and that the MEMORY screen reports plausible free space.

## Input peripherals

- [ ] Emulate the CD-i keyboard peripheral (used by authoring/industrial
  players): core-side support for the keyboard port and protocol, then host
  keyboard passthrough behind a Settings toggle. The Settings peripherals UI
  already reserves a section for it.

## Compatibility testing

- [ ] Test a broad selection of CD-i discs.
  - [ ] Build an automated headless test pass that lets AI exercise titles,
    capture screenshots/audio/diagnostics, detect stalls or panics, and record
    reproducible results before finer manual testing.
  - [ ] Keep a title compatibility matrix with region, PAL/NTSC setting, DVC
    requirement, boot/gameplay status, audio/video/input issues, and the last
    tested revision.
  - [ ] Follow automated coverage with focused manual passes for controls,
    timing, audio synchronization, display composition, and full gameplay
    transitions.
  - [ ] Never add commercial disc images or extracted title assets to the
    repository.

## Photo CD player

- [x] Incorporate the Photo CD player functionality under
  `/Volumes/Projects/Coding/Photo CD player`.
  - [x] Detect Photo CD disc images, including the root-level `CDI` directory
    found on images under `/Volumes/Projects/Coding/Disc Images/Photo CD`.
    (Decoder absorbed as `cdi-photocd`; detection is content-based on a
    frontend worker thread.)
  - [x] Distinguish Photo CD from ordinary CD-i and VCD media using disc
    contents rather than filenames alone.
  - [x] When a Photo CD is inserted, populate Photo CD interface controls in a
    panel beneath the E-Di display. (Panel appears only for discs without a
    CD-i application — when the root `CDI` directory exists, the emulated
    CD-i application drives photo display natively per the CD Bridge spec.)
  - [x] Integrate image navigation, playback/view modes, status, and
    eject/reset behavior without disrupting normal CD-i input or rendering.
  - [x] Add synthetic parser/UI tests and local, media-gated integration tests
    (synthetic INFO.PCD parser and rotation unit tests; media-gated tests in
    `cdi-photocd/tests/local_media.rs` cover a bridge disc with a CD-i
    application and the Aktuelles Berlin no-CDI exception, skipping when the
    local library is absent).

## Video CD CD-i menu and controls

- [ ] Fix two VCD faults seen with the discs under
  `/Volumes/Projects/Coding/Disc Images/VCD`. They are probably one root
  cause, in the CDIC-to-DVC stream path that VCD shares with CD-i Digital
  Video. **CD-i Digital Video is otherwise mostly correct, so avoid
  regressing it**; manual passes on both sides are needed for any change.
  - Symptom A: multi-track discs report "Your disc may be damaged or dirty"
    and never play — `CD-i See It Hear It Feel It (USA) (Kilby Predicts)
    (Rev 1)` and `Addams Family Values (USA) (Disc 1)`, in both PAL and NTSC.
    The dumps are good. Every disc that fails has more than two tracks, while
    `Addams Family Values (UK)`, with one MPEG track, reaches playback.
  - Symptom B: during playback the picture runs roughly two seconds clean,
    two seconds macroblocking, then two seconds black, cycling. `The Accused`
    is additionally off-centre with pillarboxing; the UK Addams disc shows
    the cycle without the framing error. Menus render correctly throughout.
  - Headless repro, about two minutes:
    `cargo run -p cdi-cli --release -- boot firmware/cdi220b.rom --dvc-rom
    firmware/vmpega.rom --disc <cue> --instructions 200000000 --click
    588,265 --click-at 60000000 --screenshot out.png`
  - Already ruled out, do not re-investigate:
    - Multi-track addressing is correct. `AVSEQ01.DAT` at ISO LBA 81480 maps
      to absolute frame 81630, whose sector header reads MSF 18:08:30 and
      whose payload starts `00 00 01 BA`. Pregap and per-track offsets agree
      with the Disc Xplorer model. Inspect with `cdi-cli disc <cue> --files`.
    - MPEG audio is not mis-routed into the ADPCM decoder: coding `$7F` makes
      `sector_count_for_coding` return 0 and `play_audio_data` reject it, and
      the path is gated on `audio_channel`, which the player never enables.
    - The read is not terminated by an EOF submode; there are no EOF sectors
      in the first 3000 sectors of the MPEG track.
    - Subcode Q reported a hardcoded track 1, index 1, and relative time equal
      to absolute time. Fixed in commit 7766e38; it was wrong, but it was not
      the trigger and both symptoms survive the fix.
  - Spec anchors, from the White Book (`disc specs/CD-Rom/Video CD
    Specification Version 2.0 (White Book).pdf`), Figure III.2: in
    `AVSEQnn.DAT`, MPEG video is submode `%x11x001x` (0x62) coding `$0F` and
    MPEG audio is submode `%x11x010x` (0x64) coding `$7F`, both on channel
    `$01`, with **File Number = Sequence Number = Track Number minus one**, so
    a disc with tracks 2, 3 and 4 carries file numbers 1, 2 and 3. Pause and
    margin sectors are submode `%x11x000x` (0x60), are empty, and are
    correctly filtered out today.
  - Observed shape of symptom A: the player reads about 150 MPEG sectors from
    track 2, re-seeks to the identical position, retries, then gives up — a
    read-error retry pattern, even though the sectors it received are correct.
  - Next steps:
    - [ ] Script menu navigation with `--click-event` so a run reaches real
      feature playback. Runs on the UK disc so far only reach the menu: every
      delivered sector stayed inside track 1, so the 21 KB elementary stream
      captured there was the menu clip rather than the feature.
    - [ ] With playback genuinely running, capture `--dump-vmpeg-es` and diff
      it against a contiguous extraction of the file from the disc image. This
      is the technique that localised the 7th Guest defect; see
      `docs/mpeg-dvc-plan.md`.
    - [ ] Check whether the per-sequence File Number rule above is honoured
      for discs with several MPEG tracks, since that is exactly what
      distinguishes the failing discs from the working one.
- [ ] Incorporate the CD-i menu functionality present on Video CDs under
  `/Volumes/Projects/Coding/Disc Images/VCD`.
  - [ ] Detect VCD media and its root-level `CDI` directory, which contains the
    CD-i application used to play the title.
  - [ ] Boot and preserve the disc's own CD-i menu/application behavior rather
    than bypassing it with a host-side playback shortcut.
  - [ ] When a VCD is inserted, populate VCD playback controls in a panel
    beneath the E-Di display.
  - [ ] Support play, pause, stop, seek/track navigation, status, and disc eject
    while keeping CD-i menu input functional.
  - [ ] Reuse the existing VMPEG/VCD transport and audio/video paths where the
    hardware behavior overlaps.
  - [ ] Add synthetic detection/control tests and local, media-gated playback
    tests covering menu navigation and audio/video synchronization.
