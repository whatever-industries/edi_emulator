# Contributing

## Licensing rules (read first)

- All contributions are GPL-3.0-or-later. Every source file starts with
  `// SPDX-License-Identifier: GPL-3.0-or-later`.
- Porting logic from **MAME's** CD-i files (BSD-3-Clause) is allowed; add an
  attribution comment naming the MAME source file and authors, preserve its
  copyright notice in `NOTICE.md`, and keep the applicable terms in
  `LICENSES/BSD-3-Clause.txt`.
- CeDImu is not an implementation source. No open-source license has been
  identified, so contributions must not copy or closely transliterate its
  code. Changes informed by observed behavior must be independently supported
  by specifications or license-compatible sources.
- Supported system/DVC firmware may be distributed in `firmware/`. Never
  commit game disc images or excerpts of commercial-title media (including in
  test fixtures). Synthetic fixtures only.

## Practical notes

- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` must not require external firmware or disc images.
- ROM-gated integration tests are `#[ignore]`d and keyed on `CDI_ROM_DIR`.
- The emulation core (`cdi-core`, `cdi-scc68070`) must stay deterministic:
  no wall-clock, no RNG, no UI/audio dependencies.
