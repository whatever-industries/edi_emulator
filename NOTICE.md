# Third-party attribution

This project is licensed GPL-3.0-or-later (see LICENSE). It builds on the work
of the CD-i preservation and emulation community:

## MAME (BSD-3-Clause)

Portions of the emulation logic are ported to Rust with reference to the MAME
project's CD-i driver, licensed BSD-3-Clause:

- `src/mame/philips/cdi.cpp` — driver / memory map (Ryan Holtz)
- `src/mame/philips/cdicdic.cpp` — CDIC (Ryan Holtz, Vincent Halver)
- `src/mame/philips/cdislavehle.cpp` — SLAVE MCU HLE (Ryan Holtz)
- `src/mame/philips/mcd212.cpp` — MCD212 VDSC (Ryan Holtz)
- `src/devices/machine/scc68070.cpp` — SCC68070 (Ryan Holtz)

Files containing ported logic carry an attribution header naming the MAME
source file. BSD-3-Clause code may be incorporated into GPL-3.0-or-later works;
the complete BSD-3-Clause terms and preserved contributor notices are in
[`LICENSES/BSD-3-Clause.txt`](LICENSES/BSD-3-Clause.txt).

Copyright notices for the referenced MAME sources:

- `cdi.cpp`, `cdislavehle.cpp`, `mcd212.cpp`, and `scc68070.cpp`:
  Copyright Ryan Holtz.
- `cdicdic.cpp`: Copyright Ryan Holtz and Vincent Halver.
- Subsequent changes in upstream MAME: Copyright MAMEdev and contributors.

## CD-i Emulator data files (CD-i Fan)

Board and model definitions (`cdi-core/src/boards.rs`) are transliterated from
the `.brd`/`.mdl` data files distributed with CD-i Emulator (www.cdiemu.org) by
"CD-i Fan", and ROM identification rules derive from `cditypes.rul`, licensed
LGPL-2.0-or-later by the same author.

## CeDImu

CeDImu (https://github.com/Stovent/CeDImu) was consulted as a *study-only*
reference. It carries no open-source license; **no code from CeDImu has been
copied into this project**, and contributions that do so will be rejected
(see CONTRIBUTING.md).

## SingleStepTests

CPU conformance tests use the 68000 vectors from the SingleStepTests project
(https://github.com/SingleStepTests/680x0), MIT-licensed; they are downloaded
at test time, not redistributed here.

## gen2brain/mpeg (MIT)

`crates/cdi-core/src/mpeg1_video.rs` and `mpeg1_tables.rs` are a safe-Rust
translation of the MPEG-1 video decoder from `gen2brain/mpeg`, pinned at
revision `27c6f084c6ca342380c99a59a6a130b3f716e9d7`. The original project is
Copyright (c) 2022 Milan Nikolic and licensed under the MIT License. Its unsafe
Go pixel-view convenience was not translated. The complete license text is in
[`LICENSES/MIT-gen2brain-mpeg.txt`](LICENSES/MIT-gen2brain-mpeg.txt).

MPEG Layer-II audio decoding is provided by the unmodified MIT-licensed
`oxideav-mp2` 0.0.9 Cargo dependency.
