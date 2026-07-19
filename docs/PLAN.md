# CD-i Emulator Foundation — Rust Workspace Plan

## Context

Greenfield open-source Philips CD-i emulator in `/Volumes/Projects/Coding/cdi_emulator` (currently only `references/`). Decisions made with the user:

- **Language**: Rust (Cargo workspace)
- **UI**: egui + eframe (pure Rust), cpal audio, gilrs gamepads
- **License**: GPL-2.0-or-later
- **Architecture**: data-driven board/model abstraction from day one (modeled on CD-i Emulator's `.brd`/`.mdl` device-list design); only Mono-I implemented initially
- **Milestones**: M0 scaffolding → M1 boot Mono-I BIOS to player shell → M2 CDIC + CUE/BIN disc playback. DVC/MPEG and non-Mono boards out of scope (but accommodated by the architecture).

### Reference material & legal status (verified)
- **MAME** (BSD-3-Clause, verified headers — reusable as porting reference with attribution): `src/mame/philips/cdi.cpp`, `cdicdic.cpp`, `cdislavehle.cpp`, `mcd212.cpp`, `src/devices/machine/scc68070.cpp` (Ryan Holtz et al.)
- **CeDImu** (github.com/Stovent/CeDImu): **no license → study-only, never copy**; note this in CONTRIBUTING.md
- **CD-i Emulator v0.5.3b9** binary distribution at `references/cdiemu-v053b9/`: `sys/*.brd` board memory maps, `sys/*.mdl` (~30 models), `cditypes.rul` (LGPL-2.0+ — GPL-compatible, transliterate with attribution)
- Specs: `/Volumes/Projects/Coding/disc specs/` — Green Book (`cdi_may94_r2.pdf`), ECMA TR-112 pts 1–2, ECMA-119, BRIDGE10.pdf
- Test discs: `/Volumes/Projects/Coding/Disc Images/Philips CDi/` — CD Shoot (single `CDI/2352` track), Alien Gate EU (CDI + 2 audio tracks), Alien Gate USA (CD-i Ready: app data in track-1 pregap, `INDEX 00 00:00:00` / `INDEX 01 01:43:12`)
- **BIOS ROMs on hand** at `cdi_emulator/bios/` (move under gitignored `roms/` during M0): MAME `cdimono1` set — `cdi200.rom`, `cdi220.rom`, `cdi220b.rom` (512KB Mono-I main ROMs) + real SLAVE/SERVO MC68HC705C8A dumps (8KB each); `cdimono2.zip` (CDI-220 PH3, Mono-II); `cdi910.zip` (Mono-I 910); `cdi490a.zip` incl. `impega.rom`/`vmpega.rom` (MPEG DVC — future use). Byte-identical to MAME's sets, so MAME trace-diff needs no ROM juggling; MCU dumps enable eventual SLAVE LLE (HLE remains the plan for M1).

### Ground truth from references (verified against files)
`mono1.brd` (Mono-I) memory map: `$00000000` 512KB RAM planea · `$00200000` 512KB RAM planeb · `$00300000` CDIC (IRQ level 4) · `$00310000` SLAVE (level 2, vec 26) · `$00318000` null 32KB · `$00320000` NVRAM · `$00400000` sysrom 512KB · `$004FFFE0` MCD212 registers (overlays top of ROM window) · `$80000000` 68070 on-chip peripherals.

**M1 target models (Mono-I): `cdi220b` (CD-i 220 F2, slave.ver=3231, nvr 32KB) and `cdi200a` — both ROMs are on disk in `bios/`; `cdi210a` supported by the same board.** Note: CD-i 205 is *Mini-MMC* (SCC66470 video, different map) — not an M1 target.

### Reference gathering: Discord channel archive (pre-M0 side task)
Archive the CD-i emulation Discord channel with **DiscordChatExporter.Cli v2.47.3** (github.com/Tyrrrz/DiscordChatExporter):
1. Download the `osx-arm64` CLI release zip, unpack to `~/tools/DiscordChatExporter/` (self-contained; if not, `brew install dotnet` runtime).
2. User saves their Discord token to `~/.config/discord_token` (chmod 600); commands reference it via `"$(cat ...)"` so the value never enters the transcript. User supplies the channel ID (Discord: Settings → Advanced → Developer Mode, then right-click channel → Copy Channel ID); `DiscordChatExporter.Cli channels -g <guildId>` can list channels if needed.
3. Export **both** formats into `references/discord/` (add to `.gitignore` — contains other people's messages, never commit/publish): `export -c <channelId> -f HtmlDark --media` (readable, with attachments) and `-f Json` (searchable/parseable for mining technical findings).
4. Optionally re-run periodically with `--after <last export date>` to append updates.

---

## 1. Workspace layout

```
cdi_emulator/
├── Cargo.toml                  # [workspace], resolver=2, shared lints
├── LICENSE (GPL-2.0-or-later) · NOTICE.md · README.md · CONTRIBUTING.md
├── rust-toolchain.toml · .github/workflows/ci.yml
├── crates/
│   ├── cdi-scc68070/   # CPU + on-chip peripherals; deps: log, serde(optional) only
│   ├── cdi-core/       # bus, scheduler, MCD212, SLAVE, NVRAM, CDIC, boards; NO UI/audio deps
│   ├── cdi-disc/       # CUE/BIN, sectors, subheaders; pure, no emu deps
│   ├── cdi-os9/        # OS-9 module parsing (ROM detect + debugger); pure
│   ├── cdi-frontend/   # eframe app, cpal, gilrs, rfd
│   └── cdi-cli/        # headless harness (clap, png, sha2) — what CI runs
├── roms/ (.gitignored) · tests-data/ (fixtures, hashes.toml)
```

Every file: `// SPDX-License-Identifier: GPL-2.0-or-later`. Ported-from-MAME files carry an attribution comment naming the MAME source + authors.

## 2. Core abstractions (`cdi-core`)

- **Bus** (`bus.rs`): region table built from board definition; page-table fast path over low 24-bit space + special-cased `$80000000` block; RAM/ROM as direct slices, devices dispatched by `DeviceId`. 16-bit access granularity (32-bit ops = two 16-bit cycles); reads return data + wait-state ticks. Region table supports device windows punched into ROM regions (MCD212 regs overlay ROM window — last-wins ordering).
- **Device trait** (`device.rs`): `read16/write16/tick_to/reset` with `BusCtx` (scheduler handle, IRQ lines, plane-RAM access for device DMA). Devices in `Vec<Box<dyn Device>>`.
- **Timing** (`sched.rs`): **CPU-master + event scheduler** (MAME-style, not lockstep ticking). Master clock = 30 MHz crystal ticks (`u64`); CPU free-runs to next-event horizon; devices are reactive (schedule next scanline/sector/timer event, lazily `tick_to` on register access). Determinism rule: core never reads wall clock/RNG; all input via timestamped queue. This makes CI hash tests and savestates possible.
- **IRQ routing** (`irq.rs`): 68070 on-chip interrupt controller is the hub; `IrqLine` handles created from board def (CDIC→L4 autovector, SLAVE→L2 vec 26, MCD212 via INT pins); CPU samples at instruction boundaries.
- **Boards** (`board.rs`, `boards.rs`): Rust-native `BoardDef { entries: Vec<{base, DeviceKind, params}> }` / `ModelDef { board, sysrom_glob, slave_ver, nvr_size, video_std }` mirroring `.brd`/`.mdl` semantics. Ship `MONO1` + `CDI210A`, `CDI220B`, `CDI200A` as const tables (transliterated, credited in NOTICE.md). Text-format parser deferred — schema is the format, loader can be added later. `DeviceKind` open-ended so Mini-MMC/Mono-II drop in later.
- **ROM identification**: `cdi-os9` parses OS-9 module headers (sync `$4AFC`, name/type/CRC); transliterate the Mono-I subset of `cditypes.rul` rules for auto-model-detection; `--model` CLI fallback.
- **Savestates**: per-device POD state structs, `serde`+`bincode` behind default-on `savestate` feature; event queue serialized as `(when, EventId)` (targets are IDs, not closures); ROM stored as hash and revalidated.

## 3. SCC68070 core (`cdi-scc68070`) — longest-lead item

- Modules: `decode.rs`, `exec/` (alu/move/bits/branch/misc), `ea.rs`, `exceptions.rs` (68070-specific stack frames), `timing.rs` (SCC68070 cycle table — NOT 68000 timings), `periph/` (uart, timers, i2c, dma ×2ch, pic, mmu stub).
- Bus decoupling: `trait Bus68k { read16(addr, fc) -> (u16, WaitTicks); ... }` — CPU tests plug flat RAM.
- **Decode strategy: 64K-entry function-pointer table** built once at startup (Musashi/MAME approach) — auditable per-opcode handlers, fast dispatch.
- 68070 divergences to encode: 68000 user ISA + `MOVE from CCR`, own exception frame formats (format/vector word), register-configured autovectoring via on-chip PIC, datasheet microcycle timings.
- **Test strategy**: (1) Tom Harte SingleStepTests 68000 JSON vectors (MIT) in CI — architectural state only, explicit skip-list for 68000-specific frame/prefetch minutiae; (2) offline trace-diff vs MAME `cdimono1` (`cdi-cli trace-diff` subcommand); (3) timing spot-checks post-M1.
- Build order: EA+MOVE+branch+ALU (Harte green) → exceptions/IRQ → timers+UART (BIOS needs system tick; boot diagnostics print to UART) → DMA/I2C/MMU stubs hardened as ROM exercises them.

## 4. MCD212 VDSC (`cdi-core/src/mcd212/`)

- `mod.rs` (registers CSR/DCR/VSR/DDR/DCP per plane, IRQs), `ica_dca.rs` (control-program interpreter), `decoder.rs` (CLUT4/7/8, RGB555, DYUV, RL3/RL7), `compose.rs` (transparency, mosaic, plane order, backdrop, cursor), `timing.rs` (PAL/NTSC line constants).
- **Scanline renderer on scheduler events**: per-line event runs DCA, decodes both planes into line buffers, composes into a max-res (768×560) RGBA framebuffer (pixel duplication for normal-res); ICA at vblank; scanline/ICA IRQs per config.
- M1 scope: register file, ICA+DCA, **CLUT7 + DYUV** decoders, transparency/backdrop/plane order, PAL+NTSC. Deferred: RGB555, RL, mosaic, cursor fidelity — unimplemented decoders render magenta + log (visible, not crashing).
- Decoders are pure functions (`&[u8] → &mut [u32]`) with fixture-vector unit tests in `tests-data/`.

## 5. SLAVE HLE + NVRAM

- `slave.rs`: port MAME `cdislavehle.cpp` protocol — 4 byte channels at `$00310000`; reset/version query (returns model's `slave.ver`), pointer delta packets (paced from frontend input queue), config queries; responses via scheduler events with realistic latency; IRQ L2 vec 26.
- `nvram.rs` at `$00320000`: M48T08-family — 32KB SRAM (per `.mdl`) with clock registers in top bytes; file persistence (`~/.local/share/cdi_emulator/nvram/<model>.nvr`); clock seeded from host time once at power-on, then tick-driven (`--rtc-seed` for reproducible CI). Must work before first boot — BIOS stalls in clock init otherwise.

## 6. M2: disc + CDIC + audio

- **`cdi-disc`**: `cuesheet.rs` (FILE/TRACK/INDEX, `CDI/2352`, AUDIO), `image.rs` (tracks → flat LBA, MSF↔LBA with 150-sector offset, `sector_raw(lba)`, **pregap/INDEX 00 access** — required for Alien Gate USA), `sector.rs` (Mode 2 subheader: file/channel/submode, Form1 EDC check report-only), `toc.rs` (TOC/Q answers). Unit tests against the three real cue sheets (path via env var) + tiny synthetic fixtures.
- **CDIC** (`cdi-core/src/cdic/`): port structure from MAME `cdicdic.cpp` — 16KB buffer RAM at `$00300000`, command/status regs, 75 Hz sector-pump scheduler event; subheader file/channel filtering; Form1→data buffer+IRQ4, Form2 audio→ADPCM→FIFO, CD-DA→FIFO. Commands: seek, read-N, play-CDDA, stop/pause, TOC/Q.
- `cdic/adpcm.rs`: XA-ADPCM levels A/B/C mono/stereo → 18900/37800 Hz; pure + fixture tests.
- Audio: core exposes 44.1 kHz stereo ring buffer (simple resampler); frontend cpal stream drains via lock-free SPSC ring (`rtrb`); **audio is pacing authority during playback** — dynamic rate control ±0.5% on buffer fill; underrun counter in debug UI.

## 7. Frontend (`cdi-frontend`)

- `main.rs` + `emu_thread.rs`: core on its own thread; crossbeam channels for commands/input; triple-buffered framebuffer to UI. Debugger uses command/snapshot protocol (no shared mutable state) — same protocol backs `cdi-cli` scripting.
- `screen.rs`: egui texture, nearest-neighbor, aspect-correct, PAL/NTSC aware.
- `input.rs`: mouse → pointer deltas + 2 buttons; gilrs pad → pointer emulation; keyboard fallback.
- `debug/`: cpu_panel (regs/step), disasm_panel (reuse decode tables via `disasm` feature on cdi-scc68070; breakpoints), mem_panel, plane_panel (per-plane output, CLUTs, ICA/DCA log), os9_panel (module directory from emulated RAM; later trap #0 syscall trace), cdic_panel (M2).

## 8. Testing & CI

- **CI, no ROMs**: fmt, clippy `-D warnings`, workspace unit tests, Harte 68000 suite (cached download), savestate round-trip on synthetic machine.
- **ROM-gated** (`#[ignore]` unless `CDI_ROM_DIR` set): `cdi-cli boot --model cdi210a --frames N --screenshot-hash` vs `tests-data/hashes.toml`; determinism check (two identical trace runs); M2 adds disc-boot + ADPCM stream hashes.
- Early debugging aids: `--trace-file` flight recorder (ring buffer dumped on panic), UART console echoed to stdout.

## 9. Milestones & acceptance

**M0 — Scaffolding**: workspace+CI+licenses → cdi-os9 parser → board tables+bus+scheduler → CPU skeleton passing first Harte categories → `cdi-cli boot` executes from reset vector with trace.
*Accept*: CI green on clean clone without ROMs; `cdi-cli info <rom>` lists OS-9 modules and detects cdi210a; ≥100k instructions without panic; savestate round-trips.

**M1 — Boot to shell**: CPU Harte-complete (skip-list documented) → exceptions/IRQ/timers/UART → NVRAM/timekeeper → SLAVE HLE → MCD212 ICA → CLUT7/DYUV/compose → DCA+IRQs → pointer input.
*Accept*: `cdi220b.rom` (and `cdi200.rom`) reach the animated player shell; mouse drives shell pointer; PAL and NTSC modes boot; screenshot-hash test recorded; no per-frame allocations in render path.

**M2 — Disc playback**: cdi-disc (+tests on the three real images) → CDIC command/pump/data path → ADPCM+FIFO → cpal+rate control → CD-DA → CD-i Ready pregap path.
*Accept*: CD Shoot boots into gameplay with ADPCM audio; Alien Gate EU then USA launch; underruns <1/min; disc-boot hash test; savestate mid-title resumes with audio.

## 10. Licensing files

- `LICENSE` GPL-2.0-or-later; `license` field in every crate.
- `NOTICE.md`: MAME BSD-3 attribution (files + authors) for ported logic; CD-i Fan credit for `.brd`/`.mdl`/`cditypes.rul` transliteration (LGPL-2.0+); explicit statement that CeDImu is study-only/not copied (policy repeated in CONTRIBUTING.md); note that BIOS ROMs/discs are not distributed.

## Verification (end of each milestone)

- M0: fresh `git clone` + `cargo test --workspace` green without ROMs; run `cdi-cli info` on a user-supplied ROM.
- M1: run `cdi-frontend`, load cdi210a ROM, visually reach player shell, move pointer; run headless hash test.
- M2: load `CD Shoot (Europe).cue` via frontend, play with audio; run all three disc images; run ROM-gated CI suite locally.
