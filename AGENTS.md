# E-Di: Emulator Disc Interactive Agent Guide

Read these files top-to-bottom before changing the emulator:

1. `CODEX_HANDOVER.md`
2. `docs/slave-protocol.md`
3. `docs/mpeg-dvc-plan.md`

The current milestone is M3: emulate the optional 22ER9141 VMPEG Digital
Video Cartridge on the existing Mono-I/CD-i 220 model. VMPEG is the first
target; IMPEG and the CD-i 490/Mono-IV mainboard are M4 work.

## Current M3 boundary

- Philips specifications and technical notes are the primary authority.
- MiSTer `CDi_MiSTer` at commit
  `bbaf100b5b7ab02af3f5932492c4989d5f91323f` is study/test-reference only.
  It is GPL-3.0; do not translate its HDL or software into this GPL-2.0-or-later
  repository.
- The only planned direct MPEG decoder adaptation is the MIT-licensed
  `gen2brain/mpeg` video decoder at commit
  `27c6f084c6ca342380c99a59a6a130b3f716e9d7`, translated to safe Rust with
  attribution. MPEG Layer-II audio uses MIT `oxideav-mp2` 0.0.9.
- Philips source on the local FMVDemo, FPD804, and FPD805 developer discs may
  be used directly. Record provenance for any adapted code and never
  redistribute those disc images or unrelated assets.
- Keep workspace `unsafe_code = "deny"`.
- Supported system and DVC firmware may be incorporated in `firmware/`.
- Never commit game disc images, downloaded private references, or extracted
  commercial-title media. Synthetic/project-owned test streams are allowed.

## Resume point

The initial M3 implementation and The 7th Guest gameplay boot are working.
The frontend and CLI expose PAL/NTSC player configuration. BMB Karaoke 1 is an
NTSC title: it renders correctly at 60 Hz and corrupts its base MCD212 planes
when incorrectly run at PAL/50 Hz, while the hardware cursor remains clean.
Repeated same-geometry MPEG sequence headers preserve delayed reference
frames, and the MCD251 X-display position maps one 15 MHz sample to two 30 MHz
framebuffer pixels. MCD212 interlace now retains odd/even field rows instead
of bobbing each field over the whole output, and CCIR RGB 16..235 remains
internal until the frontend/PNG presentation boundary expands it to 0..255.
The live MCD212 hardware cursor is composited after that weave so animated
cursor patterns do not remain baked into retained fields, although the user
still sees combing on The 7th Guest hand and that visual issue is deferred.
Base-system compatibility also now uses Philips section-6.2 SCC68070 timing
instead of a four-clock placeholder, and CDIC sound maps interrupt after every
consumed half so CD-RTOS can refill/terminate them (Earth Command regression).
VMPEG FMV status now latches VSYNC (`ISR $0800`) on every MCD212 frame even
when the corresponding interrupt is masked. This lets the native release
routine finish after a short still/video clip; without it The Naked Gun 2 1/2
stalls on black after its copyright screen, while the corrected run reaches
the Disc 1 chapter menu.
Use the live checklist and source ledger in `docs/mpeg-dvc-plan.md`. The next
unchecked action is to resolve five rare B-picture decode failures accumulated
across The 7th Guest's five-play run (two reproduce by 700 million instructions
in the branched third play), then cover long-run A/V drift,
pause/continue, stream switching, repeated transitions, and the cake puzzle.
Update that document's status and next action whenever a checkpoint is reached.

## Required verification

Run before handoff:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
scripts/fetch-harte.sh
cargo test -p cdi-scc68070 --test harte --release -- --nocapture
```

The Harte result must remain 118,187 passed and 0 failed. Ask the user before
creating a commit unless they explicitly authorized that commit in the active
request.
