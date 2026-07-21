# Bundled CD-i firmware

Firmware images used by E-Di, plus board-level dumps retained for future
device support. [`players.yaml`](players.yaml) is the database: player
makes/models/regions, firmware files, sizes, SHA-256 hashes, roles, and
provenance.

Supported today (Mono-I system + VMPEG cartridge):

- `cdi220b.rom` — CD-i 220 F2, the default player profile
- `cdi220.rom`, `cdi200.rom` — alternate supported Mono-I firmware
- `vmpega.rom` — 22ER9141 VMPEG Digital Video Cartridge (default DVC)

Retained for future work: `cdi490a.rom` + `impega.rom` (Mono-IV with
integrated IMPEG digital video, deferred to M4), `cdi910.rom` and its
board-level chip dumps (`mcu/`, `pal/`, TC574200), and unidentified items
under `misc/` pending research.

Unsupported Mono-IV/IMPEG behavior and the HLE'd SLAVE/SERVO MCUs do not load
these dumps yet. Game discs and their assets are not part of this directory.
