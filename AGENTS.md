# E-Di: Emulator Disc Interactive Agent Guide

Read these files top-to-bottom before changing the emulator:

1. `docs/slave-protocol.md`
2. `docs/mpeg-dvc-plan.md`
3. `docs/display-geometry.md`
4. `docs/debugging-workflow.md`
5. `docs/core-compatibility-audit.md`
6. `docs/specification-research.md`
7. `TODO.md`

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
The exact historical root `2a1d9038` and a modern hybrid proved The 7th
Guest's pre-title clip, title MPEG, both post-title MPEGs, automatic gameplay
entry, and non-looping gameplay audio. Philips section-6.2 SCC68070 timing is
now enabled and its four timing-table tests are active. The accurate timing
exposed that VMPEG had incorrectly been modeled on IN5: the CD-i 220 service
manual shows CDIC and the FMV extension sharing a daisy-chained IN4 while IN5
is unused. Latched shared-IN4 ownership restores the seven-stage transition;
the deterministic 550-million-instruction trace decodes 768 video frames with
zero decoder errors and completes both MPEG program ends. Two brief black
flashes replace authored transition animations and remain a separate deferred
incident.

Use the live checklist and source ledger in `docs/mpeg-dvc-plan.md`. M3.14's
project-owned TN 088 core regressions now cover EOS+SOS, pause/continue,
abort/restart stale-picture removal, display-timed final-picture events, PES
stream switching, 128 repeated transitions, and a six-hour integer A/V clock
mapping without requiring a behavior change. The local title runner also
emits payload-free per-play counter/DCLK summaries and milestone-raster hashes.
Two exact 1.1-billion-instruction runs produced byte-identical five-play
diagnostics and summaries with zero decoder errors; the runner can compare a
new summary against a local exact baseline. Native Philips FMVDemo title gates
now cover pause/continue and an in-place multilingual audio-stream change.
Pause/continue currently stalls at the CDIC/CDFM resume boundary with command
`$24` idle; the stream change continues but records one MP2 boundary error.
The next unchecked actions are to resolve those two evidence-backed device
gaps, followed by the cake puzzle. A timestamp- or presentation-overlap-based
oracle is still required for perceptual A/V drift. M3.13 remains closed: the
historical five rejected pictures predated the shared-IN4 correction and are
retained as superseded transport evidence; do not add picture smoothing or
concealment for them.
Update that document's status and next action whenever a checkpoint is reached.

## Git and build workflow

- Commit completed work directly to `main` unless the user explicitly requests
  another branch.
- Optimized local test builds may be created from a clean local commit before
  it is pushed, so manual testing can validate that exact revision.
- Push `main` only at a manually accepted stable checkpoint or when the user
  explicitly requests a push. Do not push every iterative commit.
- If a temporary branch is necessary, merge it into `main`, push `main`, and
  delete the temporary local and remote branch before handoff.
- Build published or distributable release artifacts only after the
  corresponding `main` push succeeds. A local test application is not a
  published release artifact.
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
