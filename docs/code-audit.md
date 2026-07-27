# Code Audit and Refactoring Roadmap

Audit baseline: `31005f1` on 2026-07-26.

This roadmap separates structural changes from compatibility corrections.
Refactors must preserve observable behavior and remain small enough that a
regression can be attributed to one responsibility boundary. Device behavior
changes continue to use `docs/debugging-workflow.md`.

## Current shape

| Area | Size | Risk | Main concern |
| --- | ---: | --- | --- |
| `cdi-frontend/src/main.rs` | 4,722 lines after the first extraction | Medium | UI, input, storage, worker control, and application state remain coupled |
| `cdi-core/src/dvc.rs` | 2,134 lines | High | VMPEG registers, transport, decode state, and A/V scheduling share one state machine |
| `cdi-core/src/mcd212.rs` | 1,711 lines | High | Timing, planes, cursor, compositing, and diagnostics are tightly ordered |
| `cdi-core/src/machine.rs` | 1,517 lines | High | Scheduling, bus mapping, DMA, interrupts, and device ownership meet here |
| `cdi-cli/src/diagnose.rs` | 1,014 lines | Medium | Incident storage, capture, comparison, reporting, and promotion share one module |

The frontend has 74 direct `Shared` mutex acquisitions. That is not itself a
correctness bug, but it makes ownership and frame-consistency difficult to
review. Replacing these piecemeal would be riskier than first defining an
immutable UI snapshot and a typed command channel.

## Important audit finding

`presentation::display_aperture` still detects non-black pixels near the bottom
of a 525-line frame and changes the vertical aperture. That is content-based
presentation behavior, conflicts with the specification-driven geometry rule
in `docs/display-geometry.md`, and is specifically listed as something not to
restore in `TODO.md`.

Do not silently remove it during a structural refactor: it affects current
title framing and needs a diagnostic incident with before/after captures,
hardware-derived replacement behavior, and neighboring display regressions.
Until then, keep it isolated and clearly visible in the presentation module.

## Ordered refactoring

### 1. Frontend boundaries

- [x] Extract rendering, screenshot, pointer, and aspect calculations into
  `presentation.rs` without changing behavior.
- [x] Extract NVRAM path/load/write/backup operations into `storage.rs`.
- [ ] Extract library scanning, ZIP media resolution, and focused-list
  navigation behind a `LibraryModel`.
- [ ] Extract Settings panels into small view functions which mutate a
  `Prefs` value and emit typed actions.
- [ ] Replace direct UI access to individual `Shared` mutexes with one
  immutable `FrontendSnapshot` plus the existing command path. Add a
  consistency test before changing the synchronization boundary.
- [ ] Split `App::update` only after the boundaries above exist; do not move
  branches into modules merely to reduce its line count.

### 2. Diagnostic CLI

- [ ] Separate incident models and serialization from capture execution.
- [ ] Separate comparison/history classification from report rendering.
- [ ] Keep the CLI command surface stable and verify byte-stable sanitized
  records for equivalent inputs.

### 3. Core devices

- [ ] Split `Machine` bus decoding from frame scheduling only after
  diagnostic-snapshot and event-order equivalence tests exist.
- [ ] Split VMPEG register interface, input transport/demux, video decode, and
  A/V event scheduling along existing hardware boundaries. Preserve register
  write ordering and FIFO backpressure in explicit trace tests.
- [ ] Split MCD212 timing, plane decode, and final composition only with
  frame/field hashes for PAL, NTSC, compatibility modes, cursor, and external
  video.

Core refactors must not be combined with timing, register, decode, or
compatibility fixes in one checkpoint.

## Verification tiers

For a pure frontend extraction:

1. `cargo fmt --all -- --check`
2. `cargo test -p cdi-frontend`
3. `cargo clippy -p cdi-frontend --all-targets --all-features -- -D warnings`
4. Workspace tests and the full Harte suite before handoff
5. Manual smoke test: Library, Settings, title display, mouse endpoints, and
   screenshot pixels/dimensions

For synchronization or core boundaries, also compare bounded diagnostic
events, frame hashes, and audio hashes on one known-good base title and one
VMPEG title before accepting the refactor.
