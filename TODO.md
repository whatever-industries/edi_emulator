# E-Di: Emulator Disc Interactive TODO

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

- [ ] Incorporate the Photo CD player functionality under
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
  - [ ] Add synthetic parser/UI tests and local, media-gated integration tests
    (currently: rotation unit tests plus the `cdi-photocd` probe example for
    manual disc verification).

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
