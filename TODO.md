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

- [ ] Fix MPEG video presentation defects seen with the VCDs under
  `/Volumes/Projects/Coding/Disc Images/VCD`: during playback of video
  content there is a pillarbox on the right-hand side of the screen that
  flashes to black, and intermittent macroblocking. The CD-i menus display
  fine, so this looks specific to the DVC full-motion video path (decode or
  MCD212 composition), not general rendering.
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
