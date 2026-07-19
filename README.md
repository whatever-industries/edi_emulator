# CD-i Emulator (working title)

An open-source emulator for the Philips CD-i player family, written in Rust.

## Status

Early foundation. Current milestone targets:

- **M0** — workspace scaffolding, OS-9 ROM inspection, board/bus/scheduler core, SCC68070 CPU skeleton
- **M1** — boot a Mono-I player BIOS (CD-i 220 F2 / 200) to the player shell
- **M2** — CDIC + CUE/BIN disc playback (data sectors, XA-ADPCM, CD-DA, CD-i Ready)

Out of scope for now: DVC/MPEG cartridge, non-Mono-I boards (the architecture
accommodates them via data-driven board definitions).

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

Run the desktop frontend with a system ROM and an optional CUE/BIN disc:

```sh
cargo run -p cdi-frontend --release -- roms/cdi220b.rom --disc /path/to/title.cue
```

ROMs and disc images are **not** included and must be supplied by the user
(place ROMs under `roms/`, which is gitignored). ROM-gated tests are `#[ignore]`d
unless `CDI_ROM_DIR` is set.

## License

GPL-2.0-or-later. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md) for
third-party attribution.
