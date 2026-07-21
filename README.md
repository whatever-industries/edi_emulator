# E-Di: Emulator Disc Interactive

E-Di is an open-source emulator for the Philips CD-i player family, written in
Rust.

## Status

Early foundation. Current milestone targets:

- **M0** — workspace scaffolding, OS-9 ROM inspection, board/bus/scheduler core, SCC68070 CPU skeleton
- **M1** — boot a Mono-I player BIOS (CD-i 220 F2 / 200) to the player shell
- **M2** — CDIC + CUE/BIN disc playback (data sectors, XA-ADPCM, CD-DA, CD-i Ready)
- **M3** — optional VMPEG Digital Video Cartridge with MPEG-1 video and Layer-II audio

IMPEG and non-Mono-I boards remain future work; the architecture accommodates
them through typed cartridge and data-driven board definitions.

## Workspace

| Crate | Purpose |
|---|---|
| `cdi-scc68070` | SCC68070 CPU core + on-chip peripherals (standalone, bus via trait) |
| `cdi-core` | Machine: bus, scheduler, boards, MCD212, SLAVE, NVRAM, CDIC |
| `cdi-disc` | CUE/BIN disc images, sector/subheader model |
| `cdi-os9` | OS-9 module parsing (ROM identification, debugger support) |
| `cdi-frontend` | egui/eframe desktop app, cpal audio, gilrs input |
| `cdi-cli` | Headless harness: boot, trace, screenshot hashing (used by CI) |

## Building

```sh
cargo build --workspace
cargo test --workspace
```

Run the desktop frontend with an optional CUE/BIN disc:

```sh
cargo run -p cdi-frontend --release -- --disc /path/to/title.cue
```

Settings can insert/remove the DVC and switch PAL/NTSC video timing while
retaining the current disc.

Game disc images are not included and must be supplied by the user.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md) for
third-party attribution.
