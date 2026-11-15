# Third-party attribution

This project is licensed MIT (see LICENSE), with the exceptions listed below.
Releases up to and including v0.2.17 were published under GPL-3.0-or-later;
copies obtained under those releases keep that license. The project builds on
the work of the CD-i preservation and emulation community:

## MAME (BSD-3-Clause)

Portions of the emulation logic are ported to Rust with reference to the MAME
project's CD-i driver, licensed BSD-3-Clause:

- `src/mame/philips/cdi.cpp` — driver / memory map (Ryan Holtz)
- `src/mame/philips/cdicdic.cpp` — CDIC (Ryan Holtz, Vincent Halver)
- `src/mame/philips/cdislavehle.cpp` — SLAVE MCU HLE (Ryan Holtz)
- `src/mame/philips/mcd212.cpp` — MCD212 VDSC (Ryan Holtz)
- `src/devices/machine/scc68070.cpp` — SCC68070 (Ryan Holtz)

Files containing ported logic carry an attribution header naming the MAME
source file. The complete BSD-3-Clause terms and preserved contributor notices are in
[`LICENSES/BSD-3-Clause.txt`](LICENSES/BSD-3-Clause.txt).

Copyright notices for the referenced MAME sources:

- `cdi.cpp`, `cdislavehle.cpp`, `mcd212.cpp`, and `scc68070.cpp`:
  Copyright Ryan Holtz.
- `cdicdic.cpp`: Copyright Ryan Holtz and Vincent Halver.
- Subsequent changes in upstream MAME: Copyright MAMEdev and contributors.

## cdiemu.org data files (CD-i Fan)

Board and model definitions (`cdi-core/src/boards.rs`) are transliterated from
the `.brd`/`.mdl` data files published at www.cdiemu.org by "CD-i Fan", and ROM
identification rules (`cdi-os9/src/rules.rs`) derive from `cditypes.rul`,
licensed LGPL-2.0-or-later by the same author. These two files are not
relicensed: they remain LGPL-2.0-or-later, carry that SPDX identifier, and the
license text is in [`LICENSES/LGPL-2.0-or-later.txt`](LICENSES/LGPL-2.0-or-later.txt).
The `cdi-core` and `cdi-os9` crates are therefore `MIT AND LGPL-2.0-or-later`.

## CeDImu

CeDImu (https://github.com/Stovent/CeDImu) was consulted for behavioral
comparison. No open-source license was identified, and no CeDImu source code
has been incorporated into this project (see CONTRIBUTING.md).

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

## CDIC Black Box Analyzer (MIT)

The hardware captures and register tests in
[`Slamy/CDIC_BlackBoxAnalyzer`](https://github.com/Slamy/CDIC_BlackBoxAnalyzer),
pinned for the CDIC audio-buffer investigation at revision
`e861f76ece477b3aff0e9c4c70f5c6ba1715e60a`, are used as independent Mono-I
behavioral evidence. No analyzer source code is incorporated into E-Di.

## Bundled firmware

The ROM images in `firmware/` are third-party firmware (Philips and others).
They are not covered by this project's MIT license; see
[`firmware/README.md`](firmware/README.md) and `firmware/players.yaml` for
provenance.
