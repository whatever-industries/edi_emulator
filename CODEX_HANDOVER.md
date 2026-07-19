# Handover: CD-i Emulator (Rust) — state as of 2026-07-18 (late)

You are picking up an open-source Philips CD-i emulator, written in Rust, at
`/Volumes/Projects/Coding/cdi_emulator`. Read `docs/PLAN.md` first — it is the
approved architecture plan (crate layout, timing model, milestones, acceptance
criteria). This file tells you what is already done and what to do next.

## ⭐ M1 CORE ACHIEVED: the player shell renders

`cargo run -p cdi-cli --release -- boot roms/cdi220b.rom --instructions
150000000 --screenshot out.png` produces the full Philips player shell
(PHILIPS logo, "Compact Disc Interactive", PLAY CD-I + pointer, INFO/
SETTINGS/DIM/MEMORY/OPEN buttons) — see `docs/shell-cdi220b.png`.
`cdi200.rom` renders identically. Deterministic boot hash recorded in
`tests-data/hashes.toml`, checked by `scripts/verify-boot.sh`.

Additions beyond the earlier sections:
- **MCD212 pixel pipeline** (in `crates/cdi-core/src/mcd212.rs`): per-line
  `process_vsr` (CLUT4/7/8/77, DYUV with half-step interpolation, RGB555 on
  plane B pairing plane A bytes, RLE, mosaic), matte/weight arrays
  (`update_matte_arrays`), plane mixing with weight factors + backdrop +
  TCR_DISABLE_MX, PAL Standard borders (24 px sides / 20 lines top+bottom),
  hardware cursor with blink. Framebuffer 768×560 0RGB
  (`framebuffer()`/`visible_size()`); rendering gated on DCR DE bit.
  IMPORTANT: MAME's `^1` plane-RAM index swizzle is NOT ported — our plane
  RAM is CPU byte order (see module doc comment).
- **cdi-cli boot** gained `--screenshot <png>` and `--hash` (SHA-256 of the
  visible framebuffer, big-endian pixel bytes).
- **eframe frontend works** (`cargo run -p cdi-frontend --release --
  roms/cdi220b.rom`, optionally followed by `--disc <cue>`; no ROM arg → file
  picker): emu thread paced to 50/60 fps
  by MCD212 frame count, texture upload per frame, mouse hover mapped to
  SLAVE pointer coords (0..767 × 0..559), left/right buttons. Status bar
  shows emulated fps. Decoded 44.1 kHz stereo is delivered to cpal through
  an `rtrb` SPSC ring.

## M2 status: CD Shoot boots; disc and frontend audio paths work

`cdi-disc` is complete and validated against all three real images
(`cdi-cli disc <cue>` prints TOC + finds the CD-i label):
- CUE/BIN with multi-file redump layout; absolute-frame addressing (LBA 0 =
  abs 150); implicit 150-frame pregap when track 1 lacks INDEX 00.
- **CD-i Ready**: Alien Gate USA's track-1 pregap data is ECMA-130
  scrambled in the rip; `read_sector_data` auto-descrambles (sync present +
  invalid header → descramble → valid). The pregap headers claim
  physical+150 (label at physical 16 says 00:02:16). MAME-compatible LBA
  mapping: `abs = lba + track1.region_start` (see `Cdic::set_disc_layout`).

`cdi-core/src/cdic.rs` is a full port of MAME cdicdic.cpp: registers at
$303C00/$303FF4+, byte-lane word assembly, 75 Hz sector pump with spinup,
Mode-2 file/channel filtering, ping-pong buffer delivery + subcode-Q with
real CRC, TOC synthesis, XA-ADPCM (4/8-bit, mono/stereo, 18.9/37.8 kHz)
with attenuation matrix wired from the SLAVE 0xC0 commands, CD-DA, audio
map (Z-buffer) decoding. Audio lands in `Machine::take_audio()` as 44.1 kHz
stereo (naive resample). CDIC DMA port (0x3FF8) copies via the 68070 DMA
ch0 registers. IN4 iack returns the CDIC's programmed vector.
`cdi-cli boot --disc <cue> --click 588,265` scripts a PLAY CD-I click.

**PROVEN working chain** (CD Shoot, cdi220b): click PLAY CD-I → driver
seeks MSF 00:02:16 → CDIC delivers the label sector → IRQ (vector 0x80) →
driver DMAs the 2048-byte payload to RAM (content verified byte-perfect:
`01 "CD-I " ... "CD-RTOS "`) → driver accepts it and initiates disc play.

**TITLE BOOT VERIFIED**: ch2 `0x8A` now requests a host-only reset preserving
SLAVE state. The restarted BIOS receives retained-mode `B0 00 42 15`, follows
with the `B1` disc-base query, loads the application, enters CD Shoot's
in-title control/gameplay screen, starts Mode-2 streaming, and produces
ADPCM audio. A second necessary fix models SCC68070 DMA status as
write-one-to-clear and software-start as setting COC; previously the label
DMA worked once, then the BIOS waited forever before its next transfer.

**SLAVE protocol decoded via firmware disassembly**: see
`docs/slave-protocol.md` (findings) + `docs/slave-zx405042p.disasm.asm`
(full disassembly, regenerate with `scripts/hc05dis.py`). Headline: the
SLAVE physically resets the 68070 (port C bit 2, routine 0x0F0C) when the
armed flag state is commanded — the BRA-to-self at ROM 0x400636 is the
boot stub *waiting to be reset*. SLAVE RAM flags survive the reset and
tell the fresh boot to play the disc. The MAME-ch2 command dispatch at
firmware 0x0512..0x0620 is decoded, including the observed `0x8A` direct
jump to the reset routine. BIOS `cdapdriv` disassembly established the
post-reset `B0`/`B1` response sequence; `0x3B` is not the launch signal.
The project owner has stated Philips relinquished firmware copyright
(recorded in NOTICE.md), so disassembly artifacts live openly in docs/.

**Original analysis (pre-disassembly)**: the disc-play flow restarts the kernel and drives
the CD *transport* through undocumented SLAVE commands: ch3 0xF8/0xF9/
0xFC/0xFD/0xFE, ch2 0x81/0x8A-0x90/0xFF/0x0A, ch0 0x83/0x8B. Sequence
ends with ch2 0x8A + BRA-to-self idle at ROM pc 0x400636 (second boot
reaches "blue screen" stage, waits ~4 s — spinup timeout? — retries, ends
at ch2 0x8A again). **MAME's cdislavehle has the same gap** (these are all
"unknown register" there too), CeDImu has no Mono-I SLAVE at all, and
cdiemu.org publishes no protocol docs. Current code treats ch3 0xF8 as
polling-disable and echo-acks 0xF9/0xFB-0xFE without IRQ (harmless, didn't
unstick). Attack plan, in order of promise:
1. **Disassemble the real SLAVE ROM** `roms/zx405042p__cdi_slave_2.0...`
   (8 KB, MC68HC705C8A): find the host-command dispatch to learn the ch2
   0x8x transport semantics and expected responses. This is the definitive
   fix and also enables eventual LLE.
2. Compare against MAME behavior directly (install MAME, run cdimono1 with
   these ROMs + a CHD of CD Shoot, trace slave_w) to see whether/how MAME
   actually boots titles from the shell click.
3. The 68070 SLAVE *driver* module in the BIOS (`sldriv` per cdi-cli info)
   can be disassembled from our side to infer expected responses.
Useful notes: uart-loopback probe at $301400 must read 0x12341234 (fixed —
enables the ROM's UART boot diagnostics, steps 1-9 print). The boot stub's
fatal/idle loop is `BRA.s self` at 0x400636 right after writing ch2 0x8A.

## What to do next (older list)

1. ~~Verify pointer interaction~~ DONE — user-confirmed in the live
   frontend: shell boots, menu behaves normally, mouse aligns with the
   shell pointer. Two fixes were needed and are landed: (a) SlaveHle
   assigns absolute pointer coords directly (the readback packets are
   absolute; delta integration baked the initial hover position in as a
   permanent offset), and (b) the frontend maps the mouse onto the active
   picture area (excluding the 24px Standard-mode borders, rescaled
   720→768) using border/active_width published per frame.
2. NTSC path: boot with a `--model` override forcing NTSC (video standard
   currently defaults PAL for all models; consider a `--ntsc` flag flowing
   into ModelDef/Mcd212/SlaveHle video_status).
3. Savestate round-trip test (serde plumbing exists, no Snapshot format yet).
4. Begin M2: `cdi-disc` CUE/BIN + CDIC (see plan §6). The CDIC uses IN4 with
   its own vector — hook `Peripherals::iack` level 4 to the CDIC when built.
5. SCC68070 datasheet timing pass (cycle counts are still ~4/insn
   placeholders; fine for now, revisit before audio sync work).
6. Note: the pointer/slave change altered input-dependent runs but NOT the
   headless no-input boot; `scripts/verify-boot.sh` hash remains valid.

## Previous status: BIOS boots CD-RTOS to its idle loop

Done since the M0 section below was written:
- **68070 on-chip peripherals** (`crates/cdi-scc68070/src/periph.rs`): LIR /
  PICR1/2 interrupt controller (faithful to MAME `update_ipl`/`iack_r`:
  IN2/IN4/IN5/NMI pins at fixed levels 2/4/5/7 with device vectors —
  IN2=SLAVE autovector 26; INT1/INT2 latched via LIR; on-chip sources vector
  `0x38+level`), timer 0 (CLKOUT/96 up-counter, reload on overflow), UART
  (TX drains to `tx_out`, echoed by cdi-cli as the boot console). The CPU now
  fetches vectors via `Bus68k::iack(level)` at acknowledge time (default
  autovector; `MachineBus` routes to the PIC).
- **MK48T08 timekeeper** in `machine.rs`: 8 KB SRAM on EVEN bytes of the
  $320000 window (MAME umask16 0xff00), device offset = window offset / 2,
  BCD clock regs at 0x1FF8-0x1FFF seeded deterministically (1995-06-13).
- **SLAVE HLE** (`crates/cdi-core/src/slave.rs`), ported from MAME
  cdislavehle.cpp: 4 channels on odd bytes at $310000 (channel =
  (addr>>1)&3), command set (0xF0 revision → $32 $31 from the model's
  `slave_version` hex pairs, 0xF3/F4/F6 queries, 0xF7 input polling, 0xB0
  disc status, audio attenuation, LCD), delayed responses raising IN2,
  first-response-byte read de-asserts, 60 Hz pointer polling with delta
  tracking (`set_pointer` is the frontend hook).
- **MCD212 stage 1** (`crates/cdi-core/src/mcd212.rs`): full register window
  (channel 2 at +0x00, channel 1 at +0x10, CSR reads at +0x01/+0x11), DA/PA
  status bits driven by per-line timing (PAL 32/312, NTSC 22/262), ICA/DCA
  control-program interpreters (STOP/NOP/RELOAD DCP/VSR/params/INTERRUPT +
  register writes 0x80-0xDC incl. CLUT banks), IT1/IT2 flags with INT1
  assertion, CSR2 read-to-clear. **ICA start address alternates 0x200/0x202
  with field parity** — remember this when writing tests. No pixels yet.
- Machine wiring: `Machine::step` = CPU step → periph/slave/mcd212 tick →
  IRQ line sync. `take_uart_output()` drains the serial console.

**Observed behavior:** `cargo run -p cdi-cli --release -- boot
roms/cdi220b.rom --instructions 40000000` boots CD-RTOS: passes the DA-bit
display-sync gate at ROM pc 0x400f44, services 700+ interrupts (system tick
+ video), transmits on the UART, and settles in the kernel idle loop around
pc 0x40a0e6. Remaining unimplemented accesses are CDIC probes ($301400
uart-loopback region).

## What to do next (M1 completion)

1. **MCD212 stage 2 — pixels**: framebuffer (768×560 RGBA in `Mcd212`),
   per-line VSR fetch + decode CLUT7 and DYUV (DDR FT bitmap modes),
   transparency/backdrop/plane order composition (port from mcd212.cpp
   `process_vsr`/`mix_lines`; pixel decoders as pure functions with unit
   tests). Then `cdi-cli boot --frames N --screenshot-png/--hash` and check
   the player shell appears (it draws via plane A DYUV background + plane B
   CLUT7 UI).
2. eframe frontend: texture from framebuffer, mouse → `slave.set_pointer`
   (note: SLAVE pointer packets are enabled by the BIOS 0xF7 command).
3. The BIOS may need more SLAVE/CDIC responses to leave the attract screen —
   watch RUST_LOG=trace for unimplemented accesses.
4. Keep Harte at 0 failed; keep clippy at 0 warnings.

## Status: Milestone M0 (foundation) is COMPLETE and verified

- Cargo workspace, 6 crates: `cdi-scc68070` (CPU), `cdi-core` (bus/scheduler/
  boards/machine), `cdi-disc` (stub until M2), `cdi-os9` (OS-9 module parser +
  ROM identification), `cdi-frontend` (stub until M1), `cdi-cli` (headless
  harness). GPL-2.0-or-later, SPDX header on every file, `unsafe_code = deny`.
- `cdi-os9`: parses OS-9 module headers (sync $4AFC, header parity, CRC-24
  poly $800063 / magic $800FE3) and identifies ROMs via rules transliterated
  from `cditypes.rul`. IMPORTANT discovery: the numeric conditions in that
  rule file (`video #>=51`) compare the module EDITION field (verified against
  real ROMs; cdi220b.rom's `video` driver is edition 51, revision 0).
- `cdi-core`: Mono-I board as data (`boards.rs`, transliterated from
  `references/cdiemu-v053b9/sys/mono1.brd`), page-table bus with sub-page
  device windows (MCD212 regs at $4FFFE0 sit ABOVE the 512KB ROM which ends at
  $480000 — not inside it), deterministic event scheduler (30 MHz tick base,
  serializable queue), NVRAM byte-RAM stub, `Machine` = CPU + bus. Reset
  mirrors the first 8 ROM bytes to RAM 0 so vectors come from ROM (SSP $1500,
  PC $4004B8) — same trick MAME uses.
- `cdi-scc68070`: FULL 68000 user ISA interpreter plus 68070 specifics:
  MOVE from CCR ($42C0), privileged MOVE from SR, 68010-style short exception
  frames WITH format/vector word (RTE pops it), trace (T latched at
  instruction start, taken after unless the instruction faulted or STOPped),
  address error on odd flow-control targets via `Cpu::set_pc_checked`.
  Undocumented flags match MAME's microcode ALU exactly (see `alu_abcd8`/
  `alu_sbcd8` in MAME `src/devices/cpu/m68000/m68000.h`): ABCD high-correction
  threshold is `> 0x9f`, carry = `r & 0x300`, SBCD's −0x60 keys off the
  UNCORRECTED borrow (`r1 & 0x100`), NBCD = SBCD with dst=0; DIV overflow sets
  N=1 Z=0 V=1; CHK clears Z(V,C) per MAME. Cycle counts are placeholders
  (~4/insn); the SCC68070 datasheet timing pass is future work.
- Conformance: SingleStepTests/m68000 vectors (custom .json.bin binary format,
  parser in `crates/cdi-scc68070/tests/harte.rs`).
  **Current result: 118,187 passed, 0 failed, ~191k skipped.**
  Skips are by-design: any test where our core takes an exception (68070 frame
  formats differ from 68000) or performs an odd-address 16-bit access; plus
  skip-listed files TAS/TRAPV (bad vectors per repo README) and RTE (format
  word). DO NOT let the failed count regress from 0.
- `cdi-cli info <rom>` lists modules + identifies the model;
  `cdi-cli boot roms/cdi220b.rom` executes 100k BIOS instructions from reset
  without panic (ends polling device stubs — expected until M1 devices exist).

## Commands

- `cargo test --workspace` — all green, no ROMs needed.
- `scripts/fetch-harte.sh` then
  `cargo test -p cdi-scc68070 --test harte --release -- --nocapture`
  — CPU conformance (vectors land in gitignored `tests-data/harte-68000/`).
- `cargo clippy --workspace --all-targets` — currently ZERO warnings; keep it.
- `cargo fmt --all` before finishing.
- Git: repo initialized, everything staged, **no initial commit yet** — ask
  the user before committing.

## Resources on disk (never commit ROMs/discs/references)

- `roms/` (gitignored): cdi200.rom / cdi220.rom / cdi220b.rom (Mono-I,
  byte-identical to MAME's cdimono1 set — trace-diff against MAME needs no ROM
  juggling), cdimono2/cdi910/cdi490a zips (490a contains MPEG DVC ROMs),
  SLAVE/SERVO MC68HC705C8A dumps.
- `references/cdiemu-v053b9/` (gitignored): CD-i Emulator binary dist by
  "CD-i Fan". `sys/*.brd` + `sys/*.mdl` are the hardware ground truth
  (memory maps, slave versions); `cditypes.rul` is LGPL (attribution already
  in NOTICE.md).
- `/Volumes/Projects/Coding/disc specs/Philips CD-i/` — Green Book
  (`cdi_may94_r2.pdf`, THE spec: MCD212/CDIC/CD-RTOS behavior), BRIDGE10.pdf,
  CD-i Ready notes. ECMA TR-112 pts 1–2 one level up.
- `/Volumes/Projects/Coding/disc specs/Philips CD-i - icdia-site-documents-2026-07-18/`
  — full mirror of icdia.co.uk incl. ~187 PDFs of Philips developer technical
  notes (display sync TN#063, CLUT restrictions TN#046, CD-RTOS bugs TN#062,
  memory allocation TN#087, player compatibility TN#085, NVRAM TN#068, player
  config TN#057, 910/MonoI status TN#069...). Mine these when implementing
  MCD212/SLAVE/CDIC details.
- Test discs: `/Volumes/Projects/Coding/Disc Images/Philips CDi/` — CD Shoot
  (single CDI/2352 track), Alien Gate EU (CDI + audio tracks), Alien Gate USA
  (CD-i Ready: app data hidden in track-1 pregap INDEX 00).

## Legal rules (enforced in CONTRIBUTING.md / NOTICE.md)

- MAME CD-i code is BSD-3-Clause: port freely WITH attribution comments naming
  the source file (cdi.cpp, cdicdic.cpp, cdislavehle.cpp, mcd212.cpp,
  scc68070.cpp — Ryan Holtz et al.) and keep NOTICE.md current.
- CeDImu (github.com/Stovent/CeDImu) has NO license: study-only, never copy.
- Never commit ROMs, disc images, or excerpts (synthetic test fixtures only).

## Next: Milestone M1 — boot cdi220b.rom to the animated player shell

Build order (see docs/PLAN.md §3–5, §9 for detail and acceptance criteria):
1. 68070 on-chip peripherals at $80000000 (port from MAME scc68070.cpp):
   interrupt controller (LIR/PICR) first, then timers (T0 system tick) and
   UART (BIOS prints boot diagnostics — echo TX to stdout in cdi-cli; huge
   debugging win). Registers are odd-byte addressed ($80001001...).
2. NVRAM timekeeper (M48T08-style clock regs in top bytes; BIOS stalls in
   clock init without it). File persistence + `--rtc-seed` for determinism.
3. SLAVE MCU HLE at $310000 (port protocol/state machine from MAME
   cdislavehle.cpp): reset/version query returns "3231", pointer packets,
   IRQ level 2 vector 26 via scheduler-delayed responses.
4. MCD212 VDSC (port register/ICA/DCA semantics from MAME mcd212.cpp):
   scanline renderer on scheduler events, ICA at vblank / DCA per line,
   CLUT7 + DYUV decoders (enough for the shell), transparency/backdrop/plane
   order, PAL+NTSC timing, display IRQs. Unimplemented decoders render
   magenta + log, never panic. Pixel decoders as pure functions with unit
   tests.
5. Wire pointer input; then eframe frontend (framebuffer texture) OR verify
   headlessly first via `cdi-cli boot --frames N --screenshot-hash`.

M1 accept: cdi220b.rom (and cdi200.rom) reach the animated player shell;
mouse moves the shell pointer; PAL and NTSC boot; screenshot-hash test
recorded; Harte suite still 0 failed.
