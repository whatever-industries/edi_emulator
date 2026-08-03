# M3 VMPEG / Digital Video Cartridge plan and source ledger

Status date: 2026-08-02

## Current implementation status

- [x] M3.1 optional DVC firmware identification and attachment
- [x] M3.2 VMPEG memory map, initial registers, shared-IN4 interrupt arbitration, and SCC68070 DMAREQ2 transport
- [x] M3.3 incremental MPEG-1 system-stream demultiplexing
- [x] M3.4 MPEG-1 video and Layer-II audio decoding
- [x] M3.5 MCD212 external-video composition and mixed audio
- [x] M3.6 CLI/frontend controls and diagnostics
- [x] M3.7 initial headless The 7th Guest boot/gameplay acceptance
- [x] M3.8 repeated-sequence reference continuity and external-video centering
- [x] M3.9 interlaced field/base-cursor composition and CCIR presentation range
- [x] M3.10 masked FMV VSYNC status and native release-path completion
- [x] M3.11 seven-stage The 7th Guest transition compatibility reference
- [x] M3.12 reconcile SCC68070 datasheet timing with VMPEG/CDIC interrupt arbitration
- [x] M3.13 full multi-play B-picture revalidation after shared-IN4 correction
- [ ] M3.14 transition compliance and long-run regressions

The specification-driven diagnostic checkpoint adds bounded DVC error,
underflow, CDIC transport, frame/plane/raster, and audio evidence without
changing device timing. Payload-free inventories were validated locally on
The 7th Guest (VMPEG required, 368x176 stream) and a VCD (ISO `CDI`
application plus three 352x240 `AVSEQ` streams). These are investigative
inputs for M3.11 and the sustained-MPEG/VCD regression; they do not bypass the
native CDIC-to-DVC path.

The VCD inventory now also follows White Book `INFO.VCD`, `ENTRIES.VCD`,
`LOT.VCD`, and `PSD.VCD` metadata. Its synthetic Mode-2 fixture covers
selection, play, and end lists; a local Video CD 2.0 validation records 156
lists without retaining payload. This supplies authored control-flow evidence
for future native-engine incidents and does not implement host-side playback.

White Book media identification is implemented but its machine integration is
deferred. `DiscImage` recognizes the LBA-16 Mode-2 Form-1 PVD combination
`CD-RTOS CD-BRIDGE` plus `CD-XA001`, and the SLAVE can report disc type 4
instead of native CD-i type 2. Native firmware then selects `$E01000`'s
13.5 MHz sample-rate converter and applies its MCD251 origin adjustment.
Philips Interactive Engineer 96/05 explicitly states that a 352-pixel Video CD
does not fill the screen in Green Book mode and does fill it when a White Book
cartridge switches the converter. A bounded Addams Family Values USA run
confirmed that type 4 removes its one-sided right edge. Manual testing also
confirmed the earlier counterexample: the same candidate shifts Addams Family
Values UK right, while Accused Netherlands remains acceptable. The USA GUI
screenshot's additional 720-pixel crop was the frontend's existing Typical
CRT aperture, not a changed MCD212 raster. This is a regression-causing
experiment, not an accepted fix. Machine disc insertion therefore retains type
2 until the MCD251 `Xo`/`Xa` sample-origin semantics are implemented; the
media classifier, SLAVE response, and new read-only register diagnostics
remain covered prerequisites.

A second bounded type-4 comparison made that prerequisite concrete. Accused
Netherlands and Addams Family Values UK both program `Xo=65`, `Yo=26`,
`Xa=384`, and `Ya=280` with a 352x288 frame and the 13.5 MHz clock enabled.
They differ in guest-authored display/window commands: Accused uses `Xd=0`,
`Xw=7`, `Ww=345`, while Addams UK uses `Xd=16`, `Xw=0`, `Ww=352`. The
available MCD251 Technical Summary names the origin and active-area registers
but does not provide the full timing/phase equation. No origin correction is
therefore integrated from these values alone. Continue only from the complete
MCD251 timing definition or a synchronized real-hardware register/output
trace.

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

The 2026-07-25 specification pass now gives that handshaking investigation a
precise contract. Green Book R2 VII.4.4.2-VII.4.4.3 says a CIL advances through
the PCL chain as buffers fill and that a still-full PCL cannot be reused.
Green Book IX.3.3.3 and Philips TN 098 describe separate circular one-sector
video/audio chains; `/mv` and `/ma` reset `PCL_Cnt` and `PCL_Ctrl` only after
consuming a buffer. The next sustained-stream experiment must trace both guest
ownership fields and the producer/consumer transition. Slowing the 75-sector
disc clock remains invalid.

The 2026-07-26 read-only trace implements that experiment at DMA boundaries.
It treats `PCL_BufSz` as a sector count, derives the Form-1/Form-2 capacity,
discovers circular and sequential chains in main and DVC extension RAM, and
records producer fill, consumer release, reconfiguration, and overwrite-risk
events. A synthetic two-PCL ring detects reuse before release. A 650-million
instruction The 7th Guest comparator observed 1,290 fills, 1,157 releases,
1,296 VMPEG packs, and zero overwrite or decoder errors. The current Addams
Family Values USA run also recorded no full-PCL reuse. Of 464 normalized
CDIC-to-VMPEG payloads, 463 match exactly. Bounded guest-write provenance now
explains the last one: at `$22CD88` the native application intentionally
changes two SCR timestamp bytes before DMA2 submission. The MPEG audio payload
is unchanged. Its first Layer-II sync header occurs after a legal 647-byte
mid-frame prefix, exactly accounting for the old “647 errors.” These bytes are
now reported as synchronization distance rather than malformed frames. Two
150-million-instruction repeats remain deterministic with 464 packs, 215
decoded video frames, 45 decoded audio frames, no PCL overwrite, and zero
demux/video/audio/stream errors. This closes the current VCD audio divergence
without changing transport timing or decoded output.

Philips TN 088 and TN 102 also separate decoder-transition failures from that
transport issue. Pause/continue, abort/restart, EOS+SOS in one sector,
sequence changes, stale B-pictures, and redundant EOS codes each need a
project-owned synthetic regression. Green Book IX ties first/last-picture and
old-PCL-release events to display/consumption time, not merely parser input.
See `docs/specification-research.md` for the cited compliance matrix and test
order.

Frontend presentation consumes `cdi-core`'s public `DisplayGeometry`, derived
only from live MCD212 `CF`, `ST`, `FD`, `SM`, field parity, and the player
standard. Horizontal Compatibility Mode masks 24 double-resolution pixels on
each side, and 625/50 vertical Compatibility Mode masks 40 host rows above and
below the 720x480 picture. `DisplayGeometry` carries Philips TSA-003's measured
PAL/NTSC pixel aspect. Rendering, screenshots, window aspect, and pointer
mapping use the same geometry. The frontend presents the complete hardware-
defined active picture and does not simulate television overscan or choose a
crop from framebuffer contents. PAL/NTSC pixel-aspect correction is independent
of that aperture; raw-square-pixel presentation remains a Settings diagnostic.
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

The SCC68070 section-6.2 timing reconciliation is complete. The accurate
timing exposed the incorrect split interrupt model; the CD-i 220 service
manual instead proves CDIC and VMPEG share a daisy-chained IN4 while IN5 is
unused. The four timing-table tests are active, and latched shared-IN4
ownership preserves the verified seven-stage The 7th Guest transition. See
`docs/core-compatibility-audit.md` and the tracked
`scc68070-device-event-scheduling` incident.
Manual neighboring checks on the committed audit baseline also pass Earth
Command sound-map termination, The Naked Gun 2 1/2's copyright-to-menu
release, Alien Gate pointer/firing audio, Merlin's Apprentice (Europe) as a
PAL base-graphics title, and The Apprentice (USA) as an NTSC base-graphics
title. Every required neighboring-title gate now passes before scheduling
reconstruction.

The 2026-07-29 clean-NVRAM frontend pass manually verifies this checkpoint
under the accurate SCC68070 timing/shared-IN4 tree: the pre-title clip, title
MPEG, both post-title MPEG stages, the visible transition into the stairwell,
automatic gameplay entry, and gameplay audio all proceed. Three brief black
intervals are now recorded around these working stages; earlier notes that
described two entirely missing transition animations are superseded pending a
same-sequence hardware reference. One or two early audio hits in the opening
Philips clip are tracked separately.

M3.13 revalidated the historical five-picture failure after the shared-IN4
transport correction. Two identical 1.1-billion-instruction runs on
`019fb5913e60d50bcf2ede6049e47376ecb5af39` each exercised five VMPEG plays,
16,619 system packs, 7,853 decoded and presented video frames, and 25,226
decoded audio frames. Both finished with identical machine, framebuffer, and
audio hashes and zero demux, video, audio, or stream errors. Decoder errors
remain cumulative across transport resets, so the zero is not reset
accounting. The five failures were captured on the original VMPEG checkpoint
before the later shared-IN4 arbitration fix eliminated a proven PCL overwrite
path; the old result is therefore superseded under changed prerequisites. No
picture concealment, smoothing, or decoder recovery heuristic was added.

Next, add project-owned TN 088 transition regressions for EOS+SOS,
pause/continue, abort/restart stale-B-picture removal, and delayed
last-picture display. Then turn gameplay into repeatable long-run A/V-drift,
stream-switch, and repeated-transition regressions. The known cake-puzzle
freeze remains an extended compatibility gate rather than an M3 boot blocker.

The first M3.14 compliance slice is now implemented without changing device
behavior. A project-owned 16x16 I/B/B/P elementary stream exercises EOS
immediately followed by SOS across fragmented writes, abort/reset after a
delayed reference remains pending, and 128 consecutive sequence transitions.
Device-level tests prove pause does not consume current or queued pictures,
continue resumes presentation, decoder reset clears the current picture,
queued pictures, ISR state, and delayed-last-picture deadline, and the final
picture/EOD/underflow events remain presentation-timed. Separate tests switch
selected PES streams without accepting the old stream's payload or timestamp
and map a six-hour SCR/PTS interval through the shared integer 90 kHz-to-45
kHz clock without drift. All pass on the existing implementation, so no TN
088 workaround or title-specific recovery was added.

The MCD251 presentation boundary is now modeled explicitly. Due decoded
pictures are staged without changing the video generator's source, then
latched only on the next externally supplied VSYNC. A monotonic picture
generation is carried through the MCD212 external-video input, and diagnostics
count any field that samples more than one generation. The native 1.03-billion
instruction FMVDemo pause/continue scenario presented 302 pictures, resumed
disc DMA and presentation after Continue, reported zero decoder errors, and
recorded zero mixed-generation fields. Manual A/B testing confirmed that the
corrected build removes the moving-video slice while the exact pre-fix base
restores it. Both builds retain the same older A/V offset, separating that
synchronization issue from the VSYNC publication correction.

The local title runner now captures low-frequency VMPEG milestones and emits
`7th-guest-transition-summary.json`. Each play records its CPU-cycle and DCLK
bounds, cumulative-counter deltas, independent decoded-audio and
presented-video throughput estimates, underflows/errors, and a SHA-256 over
the ordered milestone raster hashes. The independent duration estimates are
not labeled as A/V drift: a play epoch can legitimately continue after one
stream ends or external video is hidden. The summary contains no MPEG payload.
The default 1.1-billion-instruction gate requires all five expected plays,
while shortened investigative runs can override `CDI_VMPEG_MIN_PLAYS`.

The timestamp oracle has now found a concrete Video CD startup error. The PES
demux parsed a complete DMA batch, overwrote `last_*_pts` for every packet,
then assigned the batch's final timestamp to its first queued decoded output.
In the bounded Addams Family Values USA scenario this selected audio PTS
78,403 and video PTS 105,035, making the first picture appear 304 ms after the
first audio sample even though both deadlines were met within one field. The
affected on-disc MPEG track begins both streams at PTS 68,999. Retaining the
first selected PES timestamp changed the video startup PTS to 68,999 and
removed 408 ms of false phase offset with zero decoder errors. Manual checks
now confirm synchronized playback in Addams Family Values, The Naked Gun
2 1/2, Pete Townshend Live, and Philips FMVDemo. Long-run drift remains a
separate extended gate; if it appears, the next boundary is per-access-unit
PTS association rather than a fixed host delay.

Two exact five-play runs on 2026-07-30 produced byte-identical milestone
diagnostics and byte-identical summaries. Both finished at CPU cycle
11,673,268,782, retained all five play epochs, reported zero cumulative
demux/video/audio decoder errors, and ended with framebuffer SHA-256
`feb8df54d3a323b50b6f540163e28d28685390836a9d77ff89d0d663f8f6ae73`.
Their summary SHA-256 is
`e8cf3d4d413388eaa4596a22840d65735bcfbe6f84188bf31241cb78e44224aa`.
Set `CDI_VMPEG_BASELINE_SUMMARY` to one local summary when running the second
trace to make exact counter, DCLK, and raster-sequence equality an executable
gate. The local artifacts remain ignored.

Two optional local-media title scenarios now drive Philips FMVDemo through its
own Play Control and Multilingual interfaces. They use instruction-scheduled
device-coordinate clicks, bounded milestone diagnostics, and no extracted
media. `scripts/test-vmpeg-pause-continue-local.sh` proves the native driver
issues Pause and Continue, then requires disc DMA and presentation to advance.
Philips `play_control.c` confirms that the application calls `mv_continue()`
followed by CDFM `ss_cont()`. CDIC BlackBoxAnalyzer trace `e861f76` and the
CD-i 220 driver show that DBUF gates delivery while the optical position
advances, `$23/$24` finish the sector under the head, and `ss_cont()` starts a
fresh `$2a` read. Those transport semantics are now modeled and covered by
unit tests.

The remaining stall separated into a documented native erratum and an
emulator defect. Philips TN 088 says `mv_pause()` may return error 246
(`E$NotRdy`) and recommends a retry; the local gate therefore makes a bounded
set of Pause attempts without adding a title-specific recovery path. Once a
Pause succeeded, Continue entered vector `$0014`: VMPEG firmware at `$e536b0`
divides by its `$e040aa` display-period register, which the emulator had left
zero. The firmware treats decoded-picture period `$a8` and player-display
period `$aa` as separate 16-bit values. VMPEG now programs `$aa` from the
45 kHz DCLK as `$0708` for PAL/25 Hz and `$05dc` for NTSC/30 Hz, preserving it
across reset and cartridge power cycles. The native gate passes: after the
paused milestone it advances from 796,092 to 1,058,376 DMA words and from 227
to 302 presented frames, with 304 decoded frames and zero demux, video, or
audio errors. Manual GUI testing confirms that the title's own Pause and
Continue controls resume both video and audio normally.

`scripts/test-vmpeg-stream-switch-local.sh` selects Japanese audio during the
running Multilingual sample and requires both video and audio decode to
continue without errors or underflow. Read-only diagnostics proved that the
FMA selector register changes before the first selected PES packet: one
complete old-stream Layer-II frame remains buffered, followed by a 63-byte
incomplete old tail, while the first selected PES begins 619 bytes into a new
frame. VMPEG now waits for that selected PES boundary, decodes complete old
frames, discards only the incomplete old tail, reacquires the selected stream,
and inserts one muted Layer-II frame for the missing partial frame. This is a
standard discontinuity concealment at the device boundary, not a host delay.
The exact 1.12-billion-instruction title gate reaches 985 video frames and
1,497 decoded audio frames plus one concealed frame, with zero demux, video,
audio, or underflow errors and an unchanged final framebuffer hash. The two
earlier register-write experiments remain recorded as contextual failures.
Manual GUI testing on 2026-08-02 confirmed that the authored language controls
switch cleanly during uninterrupted playback.

Next, run the cake-puzzle extended title gate and devise a perceptual or
timestamp/presentation-overlap A/V drift oracle. Deterministic throughput
counters alone do not establish lip sync.

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
| cdiemu.org CD-i Types and DVC pages | snapshot dated by fetch script | `cditypes.rul` LGPL-2.0-or-later; web pages reference only | VMPEG/IMPEG taxonomy, firmware signatures, optional-memory maps, legacy support limits |
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
| Philips 22ER9141 service manual | local `svcmanuals/22er9141.pdf`, §§4.3-4.5 | service reference | Confirms MPEG-1, 1 MiB extension memory, distinct audio/video/DRAM sections, 68070-fed compressed data, and decoded RGB/audio returned to the base case |
| Video CD on CD-i Release 4.1 | local `docs_sw/vcd_on_cdi_41.pdf`, pp. 7-9 | Philips/ICDIA application reference | Defines native DVC-required and dirty-disc screens, PSD startup, control behavior, and `CDI/CDI_VCD.APP` filesystem layout |

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
transfer register before starting DMA. On the CD-i 220, the CDIC and FMV
extension share SCC68070 input IN4 through a hardware daisy chain. The current
request owner remains latched until it releases the line, and interrupt
acknowledge returns that owner's programmed vector. IN5 is unused on this
base-case model. This is established by the CD-i 220 service schematic and
glossary (`svcmanuals/cdi220.pdf`, PDF p. 66 and signal descriptions), not by
title-specific ordering.

MPEG data is ISO/IEC 11172 system/video/audio carried in 2324-byte Mode-2
Form-2 sectors. Green Book video sectors normally use submode `0x62` and
coding `0x0f`; MPEG audio uses submode `0x64`. The demultiplexer must retain
pack SCR and PES PTS, select the requested elementary stream, and survive
start codes split across DMA chunks.

Timing invariants:

- MPEG SCR: 90 kHz.
- FMA DCLK: 45 kHz.
- Driver A/V offsets: 22.5 kHz.
- For synchronized disc playback, the decoder which first receives data
  establishes the SCR-to-DCLK mapping used by both audio and video. A later
  DMA arrival must not establish an independent presentation timeline
  (Green Book IX.4.6.2.2).
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
comparison is 33-bit wrap-safe. Synchronized audio and video share the stable
SCR-to-DCLK anchor established by the first decoder to receive disc data,
rather than rebasing on each decoder's independent DMA arrival. Their
system-stream parsers remain independent so one elementary stream's program
end cannot consume or reset the other stream's parser state.

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
   memory-to-DVC FIFO transfers, completion state, and shared-IN4
   daisy-chain acknowledge.
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
`$0800` must still latch and remain readable without asserting the shared IN4
request. Tying the
status event to the MCD212 frame boundary changes the former permanent black
screen into the interactive Disc 1 chapter menu in a 500-million-instruction
headless run. Unit coverage verifies masked latching, IRQ gating, and ISR
read-clear semantics.

The SCC68070 section-6.2 timing pass exposed a previously hidden interrupt
contract error. In the failing 550-million-instruction The 7th Guest trace,
CDIC filled a native PCL and entered its IN4 handler, then VMPEG incorrectly
preempted it on the separately modeled IN5 before the guest advanced the PCL
producer pointer. The VMPEG consumer released that PCL, the resumed CDIC
handler skipped the pointer update, and the next sector overwrote the same
ring position. With the service-manual shared-IN4 daisy chain and latched
owner, the identical run decodes 768 video frames and 1,141 audio frames,
presents 744 frames, completes both program ends, and reports zero
demux/video/audio errors. Repeating the run after removing an unrelated DMA0
experiment gives identical results, isolating interrupt arbitration as the
cause.

Later visual testing exposed three presentation issues and one historical
transport/decoder edge case. The MCD212 formerly bobbed each 50 Hz field over
both output rows, studio-range black 16 was presented directly as gray, and
VMPEG had been expanded to full RGB before hardware mixing. These are corrected
by interlaced field weaving and a single output-boundary range expansion.
`--dump-vmpeg-es` showed the rare remaining macroblocks are not present in a
contiguous offline extraction: during the title's branched third play, runtime
delivery diverges after 1,017,394 matching bytes and two B pictures in the
fourth sequence/GOP reject midway through their macroblocks; the complete
five-play acceptance run accumulated five rejected pictures. That trace
predates the shared-IN4 correction. Two exact current five-play reruns are
deterministic and error-free, so this evidence is retained as the superseded
transport phenotype rather than grounds for VLC-style smoothing.

Required regression commands are recorded in root `AGENTS.md`. Never add game
disc paths to tracked test configuration and ask before committing.
