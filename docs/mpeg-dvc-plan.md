# M3 VMPEG / Digital Video Cartridge plan and source ledger

Status date: 2026-07-25

## Current implementation status

- [x] M3.1 optional DVC firmware identification and attachment
- [x] M3.2 VMPEG memory map, initial registers, IRQ5, and SCC68070 DMAREQ2 transport
- [x] M3.3 incremental MPEG-1 system-stream demultiplexing
- [x] M3.4 MPEG-1 video and Layer-II audio decoding
- [x] M3.5 MCD212 external-video composition and mixed audio
- [x] M3.6 CLI/frontend controls and diagnostics
- [x] M3.7 initial headless The 7th Guest boot/gameplay acceptance
- [x] M3.8 repeated-sequence reference continuity and external-video centering
- [x] M3.9 interlaced field/base-cursor composition and CCIR presentation range
- [x] M3.10 masked FMV VSYNC status and native release-path completion
- [x] M3.11 seven-stage The 7th Guest transition compatibility reference
- [ ] M3.12 reconcile SCC68070 datasheet timing with device scheduling
- [ ] M3.13 seamless-branch B-picture recovery (five rare failures remain in the full run)

The specification-driven diagnostic checkpoint adds bounded DVC error,
underflow, CDIC transport, frame/plane/raster, and audio evidence without
changing device timing. Payload-free inventories were validated locally on
The 7th Guest (VMPEG required, 368x176 stream) and a VCD (ISO `CDI`
application plus three 352x240 `AVSEQ` streams). These are investigative
inputs for M3.11 and the sustained-MPEG/VCD regression; they do not bypass the
native CDIC-to-DVC path.

White Book media identification is implemented but its machine integration is
deferred. `DiscImage` recognizes the LBA-16 Mode-2 Form-1 PVD combination
`CD-RTOS CD-BRIDGE` plus `CD-XA001`, and the SLAVE can report disc type 4
instead of native CD-i type 2. Native firmware then selects `$E01000`'s
13.5 MHz sample-rate converter and applies its MCD251 origin adjustment.
Accused Netherlands and Addams Family Values UK confirm that classification
and register path, but also prove that enabling it before implementing the
MCD251 sample-origin semantics centers Accused while shifting Addams. Machine
disc insertion therefore retains type 2 for now; the media classifier and HLE
support remain covered for the later device-level completion.

The Accused warning corruption is a separate transport issue. A direct disc
reconstruction contains 21,268 bytes while a deterministic emulator capture
contains 18,964: exactly one 2,304-byte video PES payload is missing. CDIC
copies LBA 847 into the guest PCL buffer, but LBA 848 reuses that buffer before
the native firmware submits 847 to VMPEG. The same loss reproduces on the
pre-investigation revision, so neither the White Book classification nor the
decoder experiment introduced it. Reducing the diagnostic pump cadence from
75 to 70 sectors/s prevents the loss but is not a valid hardware correction;
the incident is deferred pending CDFM/PCL and drive-response timing evidence.

Addams Family Values USA independently exercises the same failure class. Its
complete first Philips Media clip contains 628,354 elementary-video bytes and
134 frames over 4.52 seconds; the failing VMPEG play receives only 126,598
bytes over 0.88 seconds before the native application returns to its
dirty-disc dialog. The result is identical under PAL and NTSC, and the shared
VMPEG firmware successfully initializes and decodes the 352x240/29.97 stream,
falsifying a PAL-only-cartridge explanation. Media-derived XA-Bridge type 4
improves delivery but does not prevent the dialog. Arbitrary 70/60-sector
cadence experiments show nonlinear timing sensitivity and were reverted; the
required fix remains correct 75-sector CDIC/CDFM/PCL handshaking rather than a
different nominal rate.

Frontend presentation consumes `cdi-core`'s public `DisplayGeometry`, derived
only from live MCD212 `CF`, `ST`, `FD`, `SM`, field parity, and the player
standard. Horizontal Compatibility Mode masks 24 double-resolution pixels on
each side, and 625/50 vertical Compatibility Mode masks 40 host rows above and
below the 720x480 picture. `DisplayGeometry` carries Philips TSA-003's measured
PAL/NTSC pixel aspect. Rendering, screenshots, window aspect, and pointer
mapping use the same geometry and the global `Typical CRT` or `Full signal`
presentation choice. Typical CRT exposes a fixed 360x220 viewing area only
when a 525-line title supplies the full 384x240 signal. Four-sided windowboxes
are centered; pictures which substantially reach the bottom overscan edge use
the bottom-aligned form of that same aperture. No disc name or profile enters
the decision. PAL and hardware Compatibility Mode are not host-cropped.
Raw-square-pixel presentation remains a Settings diagnostic.
Mixed-region filenames no longer force whichever standard appears first in
the label. Full source notes are in `docs/display-geometry.md`.

Exact-disc display profiles use the ordered SHA-1 hashes of distinct CUE files
instead of filenames. The compact tracked database starts with the three
Merlin's Apprentice pressings and The Apprentice USA Redump #78866. Merlin
Europe Redump #54833 remains PAL/50 Hz and The Apprentice USA remains
NTSC/60 Hz. Profiles recommend player timing only: there are no title or
filename crop profiles. Authored pixels inside the selected global
presentation area remain visible. The full
current Philips CD-i metadata DAT is fetched only from `redump.info` by
`scripts/fetch-redump-cdi-refs.sh` into ignored `references/redump-cdi/`.
The frontend library and Open dialog also accept one-disc `.zip` archives when
every non-directory member is unencrypted Store data and exactly one CUE sheet
is present. Contents are streamed to a guarded temporary directory so the
existing CD-i and Photo CD sector readers remain unchanged; compressed,
encrypted, unsafe-path, and multi-CUE archives are rejected explicitly.

Next action: reconcile SCC68070 datasheet instruction timing with
whole-machine/device scheduling while preserving the verified seven-stage
The 7th Guest transition. The exact `2a1d9038` reference and the modern audit
hybrid play the pre-title clip, title MPEG, both post-title MPEGs, enter
gameplay automatically, and avoid stuttering/looping audio. Applying the
section-6.2 timing batch while devices advance only at instruction boundaries
breaks that sequence. The audit therefore quarantines, but does not discard,
the four timing assertions. See `docs/core-compatibility-audit.md`.

After the timing/scheduling reconciliation, resolve the five cumulative
B-picture decode failures (including two captured during The 7th Guest's
branched third play), then turn gameplay into repeatable long-run A/V-drift,
pause/continue, stream-switch, and repeated-transition regressions. The known
cake-puzzle freeze remains an extended compatibility gate rather than an M3
boot blocker.

The apparent v0.1.0-to-v0.2.0 regression in The 7th Guest's post-title video
was isolated separately. Reverting v0.2.0's relative subcode-Q correction did
not restore the video, while exact v0.2.0 with blank player storage did.
Comparing the pre-reset backup with the newly generated filesystem then proved
all 970 bytes of `/nvr/7th_Guest` data identical. The meaningful difference
was the generated CSD: the broken saved environment advertised `LI=625`,
while the working live virtual player regenerated `LI=525:TV`. A physical
player cannot change its standard while retaining an obsolete hardware
descriptor, but E-Di's Settings can. The frontend now invalidates only `csd`
at startup and on virtual-hardware changes so the BIOS recreates it, preserving
all title saves. This result does not close the B-picture or
repeated-transition work above.

## Scope and provenance decisions

M3 targets the Philips 22ER9141 VMPEG cartridge on the already-supported
Mono-I/CD-i 220 F2 machine. IMPEG firmware is identified but rejected with a
clear unsupported error until M4; `cdi490a.rom` is a Mono-IV system ROM and is
not a DVC firmware image.

Philips Green Book Release 2 and the developer technical notes in the local
ICDIA mirror are authoritative. MiSTer's implementation is used only to
corroborate externally visible behavior and construct original tests. No
MiSTer GPL-3.0 implementation code may be translated into this repository.

The Philips source stored on the local FMVDemo, FPD804, and FPD805 developer
discs may be used as an implementation reference or adapted when useful. Do
not redistribute the disc images or their unrelated assets.

The safe-Rust MPEG backend decision is:

- MPEG-1 video: attributed safe-Rust translation of the incremental,
  MIT-licensed `gen2brain/mpeg` decoder. Omit its Go `unsafe` pixel-view
  convenience; retain Y/Cb/Cr planes and convert explicitly.
- MPEG Layer II: pinned MIT `oxideav-mp2 = 0.0.9`.
- Do not use `oxideav-mpeg12video = 0.0.12`: its source says the registered
  decoder/slice driver is not complete.

## Source ledger

Downloaded copies belong under ignored `references/mpeg/` and are populated
by `scripts/fetch-mpeg-refs.sh`.

| Source | Pinned version | License/use | Emulator findings |
|---|---|---|---|
| `MiSTer-devel/CDi_MiSTer` | `bbaf100b5b7ab02af3f5932492c4989d5f91323f` | GPL-3.0, study/test only | Working experimental VMPEG, register/DMA traces, test ROM sources, known A/V and 7th Guest edge cases |
| CD-i Emulator CD-i Types and DVC pages | snapshot dated by fetch script | `cditypes.rul` LGPL-2.0-or-later; web pages reference only | VMPEG/IMPEG taxonomy, firmware signatures, optional-memory maps, legacy support limits |
| `gen2brain/mpeg` | `27c6f084c6ca342380c99a59a6a130b3f716e9d7` | MIT, attributed translation allowed | Incremental MPEG-1 video decoder with I/P/B reordering and streaming buffer |
| `oxideav-mp2` | crate `0.0.9` | MIT dependency | Complete frame-level MPEG-1 Layer-II decoder and persistent synthesis state |
| MAME CD-i driver | `6effbd5b66b3062fbd1ba6c59cd1b070b5f04284` | BSD-3-Clause | Current DVC map exists but handlers return zero; not a functional decoder reference |
| Philips Green Book May 1994 R2 | local `docs/cdi_may94_r2.pdf` | specification/reference | MPEG system/video/audio formats, Form-2 layout, buffers, clocks, synchronization, external A/V |
| Philips TN 088/094/097/098/101/102/103/105 and IE 96/05 | local ICDIA mirror | specification/reference | Hardware generations, timing bases, extension RAM, EOS bugs, flying mattes, seamless branching |
| Motorola MCD251 Technical Summary | local ICDIA `docs/mcd251ts.pdf` | specification/reference | VMPEG video register map and C2PIX at twice the decoded-pixel frequency |
| Philips FMVDemo developer disc | local `sw_disc/fmvdemo/fmvdemo.bin` | direct source use; do not redistribute disc | `fmv_tools.c` and `sequence.c` show independent disc, `/mv`, and `/ma` completion; video EOI is followed by video-buffer completion and audio EOI by audio underflow |
| Philips FPD804/FPD805 developer discs | local `sw_disc/fpd804` and `sw_disc/fpd805` | direct source use; do not redistribute discs | FPD805 `fmav.c` clears distinct video/audio PCL rings, arms distinct asynchronous status blocks, starts `/mv` and `/ma` separately, and aborts both decoders before another play; FPD804 supplies broader CD-RTOS/PCL/display examples |
| Philips FMV extension/recommendation/features documents | local `docs/fmv_extension.pdf`, `docs/cdi_fmv_rec.pdf`, `authoring/fmv_features.pdf` | specification/reference | Cartridge role, external-plane behavior, title recommendations, and hardware/software division |
| VLC | current upstream, unpinned | GPL study/reference only; no code copied | Confirms VCD uses the same MPEG-1 system/video/audio family, but CD-i needs its own Mode-2 Form-2 sector transport, VMPEG registers, clocks, interrupts, and compositing |
| MAME issue #1170 / The 7th Guest `cdi_loader` lead | comments through 2025-09-04 | Issue comments reference only; the attached commercial-title module was inspected temporarily and must never be fetched, stored, or committed | Loader presence test uses CSD descriptors rather than a direct DVC register probe; details below |

Known DVC/system firmware hashes (supported binaries are tracked in
`firmware/`):

| Image | Size | SHA-256 | Meaning |
|---|---:|---|---|
| `vmpega.rom` full | 262,144 | `4aed8f33a557cec13f4267acfd9f969eafd9cccf6270f16f389a16e89eb5c6f6` | VMPEG OS-9 driver ROM, two identical halves |
| `vmpega_split.rom` / first half | 131,072 | `e8a8cabb23650b53c2cb0763c3a1409f6c0d580a52fbe8ef834d44d4021db165` | VMPEG hardware ROM payload used by MiSTer |
| `impega.rom` | 262,144 | `e3131518b608fe444e03828db91ed05f0c5065a482b2635ce982e27e0ff93ff8` | IMPEG firmware; M4 only |
| `cdi490a.rom` | 524,288 | `b27fb5388baa870cf2e7317439b14d4aa3a01f85534fa3701262392da448746a` | Mono-IV system ROM, not DVC firmware |

Firmware module signatures:

- VMPEG: `csd_fmvvm`, `fmvconf`, `vmpeg`, `fmvll`, `fmvdrv`, `madriv`.
- IMPEG: `csd_fmvimpeg`, `impeg_video`, `impeg_audio`, `fmvll`, `fmvdrv`.

### The 7th Guest DVC presence test

MAME issue #1170 eventually identified the title's small OS-9 `cdi_loader`
module and reported that it is loaded at `$27FC90`, with its executable entry
at `$27FCE4`. A temporary study copy (2,048-byte padded file, OS-9 module size
`$364`, SHA-256
`77c036ab71f58ed5ca13c46411badcb66b60f09a48ef0a3e875cf7d80fb0e66a`)
was disassembled locally but was not added to the repository or reference
fetcher.

The entry point calls one helper for descriptor IDs `$5B` (91) and `$5A`
(90). The helper opens `/nvr/csd`, reads records of at most `$20` bytes,
parses each leading decimal identifier through the first colon, and succeeds
when it finds the requested ID. Only if both tests succeed does the program
fork `cdi_t7g`; otherwise it forks `cdi_nodv`. VMPEG firmware itself contains
the corresponding CSD material, including `91:/ma:`, the `/mv` descriptor,
and a `CO#90` reference.

This is useful confirmation of the software-visible presence contract: this
title does not directly probe a magic DVC bus address. The BIOS/configuration
path must discover the cartridge firmware and expose its video/audio support
through CSD records. Our firmware-mapped cartridge already follows that path,
as demonstrated by the unmodified title choosing `cdi_t7g` and reaching
gameplay. The loader does not describe VMPEG registers, DMA, MPEG decoding, or
external-plane mixing, so it does not replace the Philips hardware sources.

## VMPEG hardware contract

CPU-visible map for the later VMPEG cartridge used by the CD-i 220:

| Address | Function |
|---|---|
| `$D00000-$DFFFFF` | 1 MiB contiguous system extension RAM |
| `$E01000` | VCD/pixel-clock control |
| `$E03000` | FMA MPEG Layer-II/DSP-facing registers |
| `$E04000` | MCD251 FMV registers |
| `$E40000-$E7FFFF` | DVC OS-9 firmware ROM; a 128 KiB dump mirrors once |
| `$E80000-$EFFFFF` | 512 KiB decode/reference RAM |

The real data path is CDIC -> main RAM through SCC68070 DMA channel 0, then
main RAM -> VMPEG through the second DMA block at `$80004040`
(DMAREQ2/DMAACK2). The driver can send an initial handful of words through a
transfer register before starting DMA. VMPEG sources share external interrupt
level 5 and return their programmed vector during acknowledge.

MPEG data is ISO/IEC 11172 system/video/audio carried in 2324-byte Mode-2
Form-2 sectors. Green Book video sectors normally use submode `0x62` and
coding `0x0f`; MPEG audio uses submode `0x64`. The demultiplexer must retain
pack SCR and PES PTS, select the requested elementary stream, and survive
start codes split across DMA chunks.

Timing invariants:

- MPEG SCR: 90 kHz.
- FMA DCLK: 45 kHz.
- Driver A/V offsets: 22.5 kHz.
- FMV timer: approximately 100.446 Hz (`90,000 / 896`).
- FMV ISR VSYNC: bit `$0800`, latched on each MCD212 frame transition even if
  masked in IER; masking gates IRQ assertion, not observable status.
- PAL maximum picture: 352x288 at 25 Hz; NTSC: 352x240 near 29.97/30 Hz.
- Picture events correspond to presentation/display scheduling, not merely
  decode completion. Audio trigger is close to DAC presentation.

Completion is per decoder, not per interleaved disc record. Philips source and
TN 098 agree that `/mv` and `/ma` have separate PCL chains and asynchronous
status. A video program-end must not synthesize an audio program-end. For a
normal video ending, EOI follows Last Picture Displayed; the video buffer event
is reported only after the displayed/queued pictures drain. Both decoders must
be aborted before starting another play, even after a normal end.

PTS-less PES packets retain the most recent valid stream timestamp. SCR/PTS
comparison is 33-bit wrap-safe and each decoder keeps a stable SCR-to-DCLK
anchor for the play rather than rebasing every packet. The video and audio
system-stream parsers are independent so one elementary stream's program end
cannot consume or reset the other stream's parser state.

The external video plane is behind the base CD-i planes. MCD212 external-video
selection, transparency, mattes, cursor, crop/window/offset, and PAL/NTSC
timing still apply. DVC MP2 audio mixes with CDIC XA/CD-DA at 44.1 kHz using
saturating stereo arithmetic.

MCD212 mixing remains in the Green Book/CCIR studio range: digital black is 16
and nominal white is 235. VMPEG YCbCr conversion therefore produces internal
studio-range RGB; only frontend texture upload and CLI PNG creation expand to
desktop 0..255. Interlace `SM=1` supplies distinct PA odd/even rows that must be
retained and woven. Noninterlace `SM=0` duplicates one field into both host
rows. The 20-line top and bottom display-file masks for 625/50 Compatibility
Mode apply in both scan modes under MCD212 tables 5-6 and 5-7; masked lines do
not consume display-file or DCA data. The same core geometry controls the
frontend aperture.
The MCD212 hardware cursor is a live overlay, not part of either retained base
field. Composite the current cursor over both progressive host rows after the
base fields are woven; baking an animated cursor pattern into alternating
fields creates visible combing as The 7th Guest's hand wags its finger.

## Implementation order

1. Add DVC ROM classification and `DvcKind`/`DvcConfig`; accept 128/256 KiB
   VMPEG, recognize IMPEG, and support attach/detach with host reset.
2. Add the optional VMPEG overlay map without changing the Mono-I board
   definition. Verify firmware discovery and extension RAM before decoding.
3. Generalize SCC68070 DMA access for channel 1 and implement paced
   memory-to-DVC FIFO transfers, completion state, and IRQ5 acknowledge.
4. Implement the VCD/FMA/FMV register state machines and an incremental
   MPEG-system demultiplexer. Keep FIFO/interrupt/timer behavior testable
   without any commercial ROM.
5. Translate and test the safe MPEG-1 video decoder; integrate
   `oxideav-mp2`; make malformed/partial input resynchronize without panic.
6. Supply a timed external frame to MCD212 and mix DVC/CDIC audio.
7. Add CLI `--dvc-rom`, MPEG counters, and optional `--dump-vmpeg-es`; add frontend Settings
   Browse/Eject/status actions that retain the disc and reset the machine.

All seven implementation steps above are present. The remaining work is
compatibility hardening, not first-device bring-up.

## Compatibility gates

Default tests use only project-owned synthetic MPEG packs. The optional real
media run uses environment variables, for example:

```sh
CDI_SYSTEM_ROM=roms/cdi220b.rom \
CDI_VMPEG_ROM=roms/cdi490a/vmpega.rom \
CDI_7TH_GUEST_CUE=/path/to/7th-guest.cue \
scripts/test-vmpeg-local.sh
```

M3 acceptance requires native `/mv` and `/ma` driver initialization,
non-black composited MPEG video, audible MP2, bounded A/V drift, and at least
one MPEG -> base graphics -> MPEG transition. Extended regressions remain
explicit: pause/continue races, stream switches, sequence-size changes,
underflow recovery, seamless branching, repeated transitions, and leaving
The 7th Guest cake puzzle. A single decoded picture is not completion.

On 2026-07-19 a clean 1.1-billion-instruction run with the CD-i 220 F2 BIOS,
the recognized VMPEG firmware, and The 7th Guest Disc 1 reached the interactive
mansion staircase with the title's hand cursor. It handled five VMPEG play
requests, presented 2,106 frames, decoded 2,141 video and 5,505 MP2 frames,
produced 6,319,645 stereo sample frames. Three video program ends and one later audio program end were
routed independently; the old `avm_play: Still busy from last play` loop did
not recur. Later cumulative error accounting showed that the run contained
five rejected video pictures across earlier reset segments, while demux and
audio remained at zero errors. After interlace/range correction its final raw
framebuffer hash is
`635ef72607c16f75c892e30f59ebca2e8e26d330da9f6cea9a6a643eef1b839e`.

The title's video stream is 368x176 and native firmware programs X=8, Y=52,
W=368, H=176. The vertical letterbox is therefore authored. MCD251 C2PIX and
Philips FPD805 centering source establish that X-display is a 15 MHz sample
position: X=8 becomes framebuffer X=16, centering the 736-pixel output inside
768 pixels. The previous x4 mapping created the observed one-sided 32-pixel
pillar. Repeated same-geometry sequence headers now retain the delayed and
reference frames. On a real 79-second elementary stream this changes the
decoder from 1,961 to all 1,982 pictures, exactly matching FFmpeg's frame count
with zero decoder errors across 22 sequence headers and one sequence end.

This satisfies the initial boot/gameplay and non-black composition gates. A
headless run proves audio production and a bounded queue, but subjective sync
and long-run drift still require capture/listening or timestamp assertions;
those remain the next explicit acceptance refinement.

The Naked Gun 2 1/2 Disc 1 is a release-path regression. Its copyright clip
contains only two decoded video frames followed by roughly ten seconds of
MPEG audio. The title then pauses/stops both streams, hides external video,
and calls the native VMPEG release routine, which polls FMV ISR for a vertical
sync before returning. FMV IER is `$2000` at that point, but VSYNC status
`$0800` must still latch and remain readable without asserting IRQ5. Tying the
status event to the MCD212 frame boundary changes the former permanent black
screen into the interactive Disc 1 chapter menu in a 500-million-instruction
headless run. Unit coverage verifies masked latching, IRQ gating, and ISR
read-clear semantics.

Later visual testing exposed three presentation issues and one remaining
transport/decoder edge case. The MCD212 formerly bobbed each 50 Hz field over
both output rows, studio-range black 16 was presented directly as gray, and
VMPEG had been expanded to full RGB before hardware mixing. These are corrected
by interlaced field weaving and a single output-boundary range expansion.
`--dump-vmpeg-es` showed the rare remaining macroblocks are not present in a
contiguous offline extraction: during the title's branched third play, runtime
delivery diverges after 1,017,394 matching bytes and two B pictures in the
fourth sequence/GOP reject midway through their macroblocks; the complete
five-play acceptance run accumulates five rejected pictures. Treat seamless
branch/reference recovery—not VLC-style smoothing—as the current next action.

Required regression commands are recorded in root `AGENTS.md`. Never add game
disc paths to tracked test configuration and ask before committing.
