# E-Di: Emulator Disc Interactive

E-Di is an open-source emulator for the Philips CD-i player family, written in
Rust.

## Status

Playable. A Mono-I player boots its BIOS to the shell, runs titles from
CUE/BIN images, and drives the optional VMPEG Digital Video Cartridge through
to full-motion video gameplay.

- **M0–M2 complete** — SCC68070 core and on-chip peripherals, MCD212 video,
  SLAVE and CDIC, and CUE/BIN disc playback covering data sectors, XA-ADPCM,
  CD-DA, and CD-i Ready.
- **M3 complete** — VMPEG cartridge with safe-Rust MPEG-1 video and Layer-II
  audio, reaching interactive gameplay in *The 7th Guest*.

Current work is title compatibility. Video CD playback has known faults:
discs with several MPEG tracks are rejected by the player application, and
playback shows cyclic artefacts — both tracked in the project's TODO. IMPEG
and non-Mono-I boards remain future work; the architecture accommodates them
through typed cartridge and data-driven board definitions.

Beyond emulation, the desktop app provides a disc library browser, mouse,
keyboard and gamepad input with rebindable controls, a Photo CD viewer for
discs that carry no CD-i application, persistent saved games, selectable
system ROMs, PAL/NTSC switching with optional per-disc region matching, and
hold-to-fast-forward.

## Workspace

| Crate | Purpose |
|---|---|
| `cdi-scc68070` | SCC68070 CPU core + on-chip peripherals (standalone, bus via trait) |
| `cdi-core` | Machine: bus, scheduler, boards, MCD212, SLAVE, NVRAM, CDIC, DVC |
| `cdi-disc` | CUE/BIN disc images, sector/subheader model |
| `cdi-os9` | OS-9 module parsing (ROM identification, debugger support) |
| `cdi-photocd` | Kodak Photo CD image pack decoder (Base/4Base/16Base) |
| `cdi-frontend` | egui/eframe desktop app, cpal audio, gilrs input |
| `cdi-cli` | Headless harness: boot, trace, disc inspection, screenshot hashing (used by CI) |

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
