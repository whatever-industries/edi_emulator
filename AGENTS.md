# E-Di: Emulator Disc Interactive Agent Guide

Read these files top-to-bottom before changing the emulator:

1. `docs/slave-protocol.md`
2. `docs/mpeg-dvc-plan.md`
3. `docs/display-geometry.md`
4. `docs/debugging-workflow.md`
5. `docs/core-compatibility-audit.md`
6. `TODO.md`

Whenever a compatibility problem is reported, open or resume a diagnostic
incident using `docs/debugging-workflow.md` before changing emulation
behavior. Prior failed attempts are contextual evidence, not permanent bans:
state what prerequisite or evidence changed before repeating an equivalent
experiment.

The current milestone is M3: emulate the optional 22ER9141 VMPEG Digital
Video Cartridge on the existing Mono-I/CD-i 220 model. VMPEG is the first
target; IMPEG and the CD-i 490/Mono-IV mainboard are M4 work.

## Current M3 boundary

- Philips specifications and technical notes are the primary authority.
- MiSTer `CDi_MiSTer` at commit
  `bbaf100b5b7ab02af3f5932492c4989d5f91323f` is study/test-reference only.
  Reimplement behavior from the specifications; do not translate its HDL or
  software into this repository (a clean-room policy, independent of license
  compatibility).
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
CDIC sound maps interrupt after every consumed half so CD-RTOS can
refill/terminate them (Earth Command regression).
VMPEG FMV status now latches VSYNC (`ISR $0800`) on every MCD212 frame even
when the corresponding interrupt is masked. This lets the native release
routine finish after a short still/video clip; without it The Naked Gun 2 1/2
stalls on black after its copyright screen, while the corrected run reaches
the Disc 1 chapter menu.
The exact historical root `2a1d9038` and a modern hybrid prove The 7th Guest's
pre-title clip, title MPEG, both post-title MPEGs, automatic gameplay entry,
and non-looping gameplay audio. The modern hybrid retains all current work but
temporarily restores the pre-regression SCC68070 timing behavior. Applying the
Philips section-6.2 timing batch while devices advance only after a complete
instruction breaks that sequence; the four timing assertions are quarantined
in the current `main` baseline, not deleted or treated as disproven. Two brief
black flashes replace authored transition animations and remain a separate
deferred incident. The slight sizzling heard in the historical executable is
absent from the modern audit hybrid.

Use the live checklist and source ledger in `docs/mpeg-dvc-plan.md`. The next
unchecked action is to reconcile datasheet SCC68070 timing with
whole-machine/device scheduling while preserving the seven-stage transition.
After that, resolve five rare B-picture failures and cover long-run A/V drift,
pause/continue, stream switching, repeated transitions, and the cake puzzle.
Update that document's status and next action whenever a checkpoint is reached.

## Git and build workflow

- Commit completed work directly to `main` unless the user explicitly requests
  another branch.
- If a temporary branch is necessary, merge it into `main`, push `main`, and
  delete the temporary local and remote branch before handoff.
- Build release artifacts only after the corresponding `main` push succeeds,
  so every delivered build comes from the published `main` revision.
- Continue asking before commits unless the active request explicitly
  authorizes them.

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
