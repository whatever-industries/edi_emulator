# Handover: E-Di: Emulator Disc Interactive (Rust) — state as of 2026-07-19

You are picking up an open-source Philips CD-i emulator, written in Rust, at
`/Volumes/Projects/Coding/cdi_emulator`. Read `docs/PLAN.md` first — it is the
approved architecture plan (crate layout, timing model, milestones, acceptance
criteria). This file tells you what is already done and what to do next.

## M3 update: VMPEG reaches The 7th Guest gameplay

Root `AGENTS.md` defines the current mandatory read order and verification
commands. For M3 details, source provenance, exact firmware hashes, and the
live checklist, use `docs/mpeg-dvc-plan.md`; the M1/M2 sections below remain
historical context.

The optional 22ER9141 VMPEG cartridge is now attached to the Mono-I/CD-i 220
model with its extension/decode RAM, firmware map, MCD251/FMA/VCD registers,
IRQ5, and SCC68070 DMA channel 1. MPEG-1 systems/PES, safe-Rust MPEG-1 video,
and Layer-II audio decoding feed the MCD212 external plane and the existing
44.1 kHz audio mix. CLI `--dvc-rom` and frontend Settings insertion/removal
support are present; IMPEG is recognized but deliberately deferred to M4.

The supported `cdi220b.rom`, `cdi220.rom`, `cdi200.rom`, and `vmpega.rom`
images are versioned in `firmware/`.
The frontend uses the bundled CD-i 220 F2 and VMPEG images by default.
Settings presents explicit Insert/Remove DVC Cartridge and PAL/NTSC controls
and retains the disc across the required reset. macOS captured input also
keeps cursor lock asserted, normalizes the hidden host cursor at capture, and
uses native NSEvent deltas as logical points so available CD-i pointer travel
does not depend on the original click position.

A 1.1-billion-instruction The 7th Guest run reached the interactive
mansion staircase after five VMPEG plays: 2,106 frames presented, 6,319,645
audio sample frames produced, and independent video/audio program ends 3/1.
Cumulative counters now correctly retain five rejected video pictures across
transport resets (demux/audio errors remain zero); the previous zero described
only the final reset segment. After the interlace/range corrections the final
raw framebuffer hash is
`635ef72607c16f75c892e30f59ebca2e8e26d330da9f6cea9a6a643eef1b839e`.
The repeatable opt-in command is `scripts/test-vmpeg-local.sh`, using the three
environment variables documented in `docs/mpeg-dvc-plan.md`.

The 7th Guest's MPEG pictures are authored at 368x176. Native `fmvdrv` writes
X=8, Y=52, W=368, H=176: the vertical letterbox is authored, while X=8 centers
the 736-pixel output at framebuffer X=16. The MCD251 C2PIX documentation and
FPD805 `fmav_center_image()` confirm that one X-display unit becomes two 30 MHz
framebuffer pixels; an old x4 conversion shifted every clip right.

Later Philips-logo evidence from The 7th Guest and Mutant Rampage showed that
uneven framing can correct itself inside one clip, so it is a live display
mode/field transition rather than two separately authored bumpers. MCD212 now
duplicates rows only in noninterlace and weaves PA odd/even fields into
alternating framebuffer rows in interlace. PAL Standard's 20-line border rule
is gated to noninterlace, matching the datasheet. Internal digital black/white
remain 16/235 through base/VMPEG mixing; the frontend and CLI PNG path expand
once to desktop 0/255. This addresses the 50 Hz bump and gray blacks without
double-expanding MPEG video.

The 7th Guest's animated hand is the MCD212 hardware cursor
(`CURCNT=$08000F` at the staircase), not an MPEG or base-plane sprite. It is
now excluded from the retained odd/even base fields and composited as a live
overlay across both host rows after field weaving. This preserves true
interlace for the scene in the renderer, but the user still observed the
finger combing after this change. Treat that as an unresolved, low-priority
cursor/display-timing issue rather than claiming the visible defect fixed.

MAME issue #1170's `cdi_loader` lead was also followed through. The title's
entry at `$27FCE4` does not probe a DVC register directly: it searches
`/nvr/csd` for decimal descriptor IDs 91 and 90, forks `cdi_t7g` only when
both exist, and otherwise forks `cdi_nodv`. The attached commercial module
was studied only in a temporary directory and is not tracked or fetched.
This validates firmware/CSD enumeration but adds no VMPEG transport or decoder
register information; full notes and its hash are in `docs/mpeg-dvc-plan.md`.

The MPEG decoder must also retain its delayed/reference frames across repeated
same-geometry sequence headers. A real 79-second stream has 22 sequence headers
but only one sequence end: resetting at every header dropped 21 pictures and
caused transient reference corruption. The corrected decoder produces all
1,982 pictures from that stream, matching FFmpeg's count with zero errors.

Rare residual 7th Guest macroblocks are now isolated from decoder arithmetic.
CLI `--dump-vmpeg-es PATH` captures the current play without host I/O in the
deterministic core. A 700-million-instruction run matches a contiguous disc
extraction for its first 1,017,394 elementary bytes, then follows the title's
branched CDIC read order and reports two rejected B pictures in sequence/GOP
4; the complete five-play run accumulates five. Error totals now survive
transport resets instead of being overwritten by the final play. Continue by
mapping that branch boundary against Philips
seamless-branch behavior and reference-picture rules, not by postprocessing
the visible blocks.

The compatibility fix came from Philips source embedded in the local FMVDemo,
FPD804, and FPD805 developer discs plus TN 098. `/mv` and `/ma` have separate
PCL chains, parser state, EOI/completion signals, and abort paths; a video
program-end must not finish audio. PTS-less packets retain the last timestamp,
SCR/PTS arithmetic wraps at 33 bits, and each decoder uses a stable play-clock
anchor. Clearing CDIC DBUF bit 14 stops sector delivery; a discarded
event-only scan heuristic contradicted that behavior and stalled the title.

The Naked Gun 2 1/2 exposed a separate native-driver release contract. Its
copyright sequence is a two-frame MPEG still followed by MPEG audio; after
stopping both decoders, the VMPEG ROM release path at `$E529F8` polls FMV ISR
for VSYNC. FMV ISR bit `$0800` must therefore latch at every MCD212 frame
boundary even while FMV IER masks its external IRQ. The bit remains readable
and read-clear. Wiring this status event to the real MCD212 frame transition
eliminates the black-screen poll loop and reaches the Disc 1 chapter menu in a
500-million-instruction headless run. A focused unit test covers masked status,
IRQ gating, and read-clear behavior.

NTSC player timing is now selectable in the CLI and frontend. This fixes BMB
Karaoke 1: the Japanese title renders correctly at 60 Hz; the earlier
scrambled base planes were caused by forcing PAL/50 Hz, while its separately
composited MCD212 cursor remained clean.

Next work is seamless-branch B-picture recovery, then compatibility hardening:
quantitative long-run A/V drift,
pause/continue, stream switching, repeated transitions, dimension changes,
underflow recovery, and the known 7th Guest cake-puzzle edge case. Do not
commit game discs/private references, and ask the user before creating a
commit.

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

**DOCUMENT-DRIVEN REGRESSIONS (2026-07-19)**: the May 1994 Green Book plus
cdi220b `cdapdriv` disassembly establish the two-layer Mode-2 filter: CDIC
delivers file-matched EOF/EOR/TRIG even across the channel mask, then the BIOS
routine at ROM `$2A804` clears cross-channel EOR/data-type flags while keeping
EOF/TRIG. Unit tests lock the low-level CDIC behavior. MCD212 table 5-10 also
showed that DCA storage always strides 64 bytes but the per-line fetch budget
is 32 bytes with CF clear and 64 with CF set; the renderer and a synthetic DCA
test now enforce that distinction. Tables 5-9 and 9-12/9-15 also require the
channel-1 DE master gate plus IC and DC for DCA execution; those gates are now
correctly enforced and tested. TN 069 section 3.3 establishes that the first
linked LCT executes immediately after the FCT. The first DCA slot is therefore
prefetched after ICA, with later slots advanced after rendered lines; a PAL test
locks this to exactly 280 slots per frame. MCD212 interrupt instructions always
set IT1/IT2, while active-high DI1/DI2 suppress only CPU-pin propagation; CSR2
reads clear IT1/IT2/BE. Those distinctions are now modeled and tested.

The literal table-5-8 non-interlaced ICA entry rule was tested but is not yet
enabled: changing the simplified scheduler from alternating `$400/$404` to a
fixed `$400` stranded both title regressions in the shell (identical hash
`beddc17aebef95c370d529f0e5cd9fcc8727402c097ee4cdd54b81f409e9517d`).
Keep the working alternating behavior until scan mode, half-lines and field
parity are modeled as one timing change.

Audio scheduling has one new Green-Book/TN-driven correction. Memory sound
maps now retain priority over incoming real-time-file ADPCM instead of being
canceled by the next disc-audio sector, and starting a sound map aborts CD-DA.
Focused CDIC tests cover both. Earth Command exposed a second sound-map bug:
after starting at Z-buffer `$2800`, the two halves `$2800/$3200` both contained
coding `$04`, but CDIC only raised completion on a `$FF` terminator. CD-RTOS
therefore never learned that a half was free and the same sound looped forever.
CDIC now raises the audio-buffer completion IRQ after every consumed half, as
MAME's low-level behavior and TN 069/079's transfer semantics require. A
headless Earth Command trace then refilled the halves, wrote `$FF` to `$3200`,
and ended normally. A focused regression locks this down; it should also fix
Alien Gate gunshots. Audible completion remains coarser than real hardware, so
a later change must still split transfer and playback cursors and model buffer
depth per machine.

The SCC68070 timing placeholder was also a real compatibility defect behind
CD Shoot's extremely fast flag and skeet animation. Nearly every instruction
previously cost four clocks and all normal bus accesses cost zero. Philips
SCC68070 April 1993 section 6.2 instead specifies a seven-clock minimum,
four-clock bus transfers, effective-address internal costs, 13/14-clock
branches, `13 + 3n` shifts, 76-clock multiply, 130/169-clock division, and
55/65-clock exception/interrupt paths. The core now accounts for those tables,
including MOVE/MOVEM, immediate/bit/control/BCD cases and bus wait states.
Spot tests lock representative table values; the Harte semantic corpus remains
cycle-agnostic and must still report 118,187/0. CD Shoot reaches its language
screen headlessly with the new timing. Have the user judge flag/skeet cadence
and sound behavior in the frontend before considering the compatibility issue
closed.

Both title regressions still boot after the first-LCT timing correction
(350,000,000 instructions, scripted shell click). Their expected final frames
changed because DCA state now applies to the intended line: Alien Gate USA
reaches its working menu
(`72072c68036fdb7417cbd4d2db5cc780748960735c9f70f8757bea2b7929cb96`),
and CD Shoot reaches its language/game screen
(`5cab5e8d846e14ee64f48aff904ca21992b48525d8f833acd0a353e7dce9e8e2`).

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
The 187-PDF local ICDIA mirror is now inventoried and the sources assessed for
Mono-I work are tracked in `docs/icdia-archive-assessment.md`.
The CD-i 220 service-manual block diagram/signal list now independently
corroborates the reset model: SLAVE `RSTOUT` starts the sequence, active-low
`RESETN` resets the other host ICs, `NRESET` resets the video/VSR domain, and
SLAVE remains outside that host reset domain. Preserve `SlaveHle` while fully
resetting the host devices.

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
   shell pointer. The frontend initially synchronizes its absolute position,
   then SlaveHle accumulates relative motion around guest-selected positions
   (F3 reports a relative device even though F7 readback packets contain the
   accumulated coordinates). This is required by titles such as Alien Gate.
   The frontend also maps the mouse onto the active
   picture area (excluding the 24px Standard-mode borders, rescaled
   720→768) using border/active_width published per frame.
   The firmware's ch3 `F8/F9` pair controls `$63.6`; it does **not** disable
   pointer polling. `F7` starts the HLE poller. `FE` changes firmware input
   mode `$58.3`, but the HLE poller must remain active: Alien Gate sends FE
   during shell handoff and continues using the pointer. This behavior is
   user-verified in the frontend, with no unintended menu activation.
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
  harness). GPL-3.0-or-later, SPDX header on every file, `unsafe_code = deny`.
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

- `cargo test --workspace` — all green, no external firmware needed.
- `scripts/fetch-harte.sh` then
  `cargo test -p cdi-scc68070 --test harte --release -- --nocapture`
  — CPU conformance (vectors land in gitignored `tests-data/harte-68000/`).
- `cargo clippy --workspace --all-targets` — currently ZERO warnings; keep it.
- `cargo fmt --all` before finishing.
- Git: repo initialized, everything staged, **no initial commit yet** — ask
  the user before committing.

## Resources on disk

- `firmware/` (tracked): supported Mono-I `cdi200.rom`, `cdi220.rom`, and
  `cdi220b.rom` system firmware plus the supported VMPEG `vmpega.rom` image.

- `roms/` (gitignored scratch area): additional unsupported model firmware,
  archives, and SLAVE/SERVO dumps. Promote only firmware useful to an
  implemented device/model into tracked `firmware/`.
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
- Supported system/DVC firmware may be incorporated in `firmware/`. Never
  commit game disc images or excerpts from commercial titles (synthetic test
  fixtures only).

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
