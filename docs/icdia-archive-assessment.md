# Local ICDIA document archive assessment

Source tree (local research material, not redistributed):
`/Volumes/Projects/Coding/disc specs/Philips CD-i - icdia-site-documents-2026-07-18`

The mirror contains 342 files, including 187 PDFs, 123 HTML files and 29 ZIP
archives. This index records what has actually been assessed so later work can
distinguish a checked source from an unchecked lead. It is not a claim that all
187 PDFs have been read cover to cover.

The local mirror is incomplete in at least one useful place: `docs/GRNBK/`
contains the HTML index but not the linked extensionless chapters or
`GRNBK.zip`. A research-only copy of that linked archive was recovered into
ignored `tmp/` storage. It is an older, searchable Green Book edition and is
useful for locating topics, but the May 1994 PDF remains the later authority.
None of these copyrighted source documents belong in the repository.

## Archive map

| Area | PDFs | Emulator relevance |
|---|---:|---|
| `notes/` | 64 | Philips/PIMA implementation notes and player quirks |
| `docs/` | 25 | Chip data sheets, input-device specifications, change notes |
| `manuals/` | 17 | User-visible model behavior |
| `authoring/` | 17 | Disc layout, real-time files, media and FMV authoring |
| `svcmanuals/` | 14 | Board schematics, signal names and service procedures |
| `microware/` | 12 | CD-RTOS/OS-9 calls, drivers and ABI behavior |
| remaining directories | 38 | Technical training, newsletters, software and proceedings |

`notes/techindex.pdf` has been extracted and reviewed as the routing index
for all technical notes. Notes are selected from their documented subjects,
not only from filenames.

## Assessed for current Mono-I work

| Source | Relevant finding |
|---|---|
| `docs/pointing_devices.pdf` | Relative devices identify as `M`, emit a three-byte signed-delta packet only on motion or button change, and must not emit stationary repeats. This supports keeping frontend motion relative and edge-driving buttons. |
| `techdocs/tech_training_cdi910.pdf`, pp. 13–14 | The 68070 controls the drive MCU through SPI via the SLAVE; disc data also returns over X-bus through DSP/CDIC. The SLAVE therefore owns drive-control/status behavior that CDIC commands alone cannot replace. |
| `docs/mcd212rev0.pdf` | Register, CLUT, transparency, region and display-control behavior used by the MCD212 implementation. |
| `docs/mcd221tsrev0.pdf` | CD interface, ADPCM/audio mixing and host-interface division of responsibility. Its memory map exposes two 2304-byte ADPCM buffers, reinforcing that transfer and audible playback are separate states on integrated later hardware. |
| `docs/cdi_may94_r2.pdf` | The later Green Book authority for display-control, sector selection, audio scheduling and CD-RTOS calls. In particular, a memory sound map overrides real-time-file ADPCM, while starting one during CD-DA aborts CD-DA. |
| `docs/cdi_ready.zip` | The 1991 tentative CD-i Ready application note: track 1 is audio; CD-i data begins at absolute time 00:00:00 inside INDEX 00; data is followed by at least 30 seconds of message sectors and at least 120 seconds of digital silence before INDEX 01. A player may distinguish the format by reading the sector whose header is 00:02:16 and validating its Disc Label. |
| `authoring/cdi_standards.pdf`, `discbuild.pdf` | Confirm the 75-sector/s stream, Mode-2 Form 1/Form 2 sizes and subheader roles. The supplied normal-disc emulator trace jumps to header 00:02:16 at absolute sector 166, independently matching the standard-disc mapping in `cdi-disc`. |
| `svcmanuals/cdi220.pdf` | The MMC block diagram separates the SLAVE CPU from the host reset domain. Its signal list defines SLAVE `RSTOUT` as starting the reset sequence high, `RESETN` as the active-low reset for the other ICs, `NRESET` as the video/VSR reset, and `HALTN` plus reset as placing the 68070 in reset. This validates a full host-device reset that preserves SLAVE state. |
| `microware/cdisys.pdf` | CD-RTOS calls and asynchronous play/status semantics, including `SS_CDDA`. |
| `techdocs/cdi605t_techdoc_r13.pdf` | Startup modes, port/input-device configuration and development-player driver context. |
| TN 046 | Later VSR CLUT loading is path-sensitive. The documented safe sequence for plane-A CLUT8 is CLUT7 on both planes, load banks 0/1 through A and 2/3 through B, then select CLUT8 on A. |
| TN 057.1 | Applications must initialize configurable hardware state; DCP/display parameters are not promised to be initialized at shell handoff. |
| TN 062, 069, 076 | Hardware and driver differences across 18x, 605/910 and consumer players. These include DCP-link timing, non-atomic two-plane updates, file-position cadence, seek quirks, old CLUT/matte defects and input-device CSD selection. TN 069 says the first linked LCT executes immediately after the final FCT instruction rather than waiting for the first active-line boundary. |
| TN 066 | TOC addresses are absolute MSF, while file positions use the logical domain with the two-second offset. |
| TN 090 | Disc reads and play calls start at the file-position pointer and include an implicit physical seek. |
| TN 092 | Direct audio transfer is autonomous after start; the CPU controls start/stop/pause/continue and applications commonly synchronize through the file-position pointer. |
| TN 094 | Disc delivery is the authoritative 75-sector/s time base; applications may poll file position to synchronize display behavior. |
| TN 063, 089, 100 | Display synchronization, scan timing and CLUT-screen techniques relevant to MCD212 compatibility. |
| TN 079 | Sound-map completion reports the transfer into a manufacturer-dependent audio-processor buffer, not the instant the last sample becomes audible. Status describes the next sector to transfer and is unsuitable for A/V synchronization. |
| TN 085.1 | Cross-player base-case requirements: fixed-but-not-exact system tick, 1–3 second seek tolerance, PAL/NTSC, multiple input devices, initialized settings, and defined/legal DCP programs. |

The PDF pages behind the current SLAVE/input conclusions were also rendered
and visually checked; text extraction was not treated as sufficient evidence.

## Current consequences

- ch3 `F7` starts HLE pointer reporting. Firmware command `FE` changes
  `$58.3`, but it cannot be modeled as stopping HLE pointer delivery: Alien
  Gate sends it at player-shell handoff and then continues using the pointer.
- A captured desktop mouse is a relative source. The title-programmed CD-i
  cursor position must survive capture, and only host deltas should move it.
- Button state changes generate packets; a capture gesture is frontend UI and
  must not become a CD-i button transition.
- Direct-disc playback and its progress/status cannot be inferred solely from
  CDIC command traffic. SLAVE/SERVO and CDIC are parallel parts of the path.
- The service schematic corroborates the reset implementation: ch2 `0x8A`
  must reset the host IC domain (not only CPU registers) while leaving SLAVE
  RAM/protocol state intact. Main RAM and NVRAM remain storage, not devices to
  be reconstructed during that reset.
- CD-i Ready layout in `cdi-disc` agrees with the Philips/Sony note: stored
  track-1 INDEX 00 starts at absolute frame zero and the data label is found at
  header address 00:02:16. Real rips may not follow every tentative duration
  recommendation, so recognition must remain content-based rather than require
  a three-minute pregap.
- The 18x notes are not a license to make every Mono-I player buggy. Several
  defects (CLUT banks 2/3, matte x=0, selected-sector loss after seek and the
  early-main-channel “twilight zone”) are explicitly listed as fixed in the 605
  and consumer generation. Compatibility quirks must be attached to a machine
  profile and enabled only when a title proves it needs them.
- CD-RTOS 1.1 audio priority is not last-writer-wins: memory sound maps retain
  priority over incoming real-time-file ADPCM, which continues streaming and
  resumes afterward. Starting a sound map during CD-DA aborts the CD-DA play.

## Disc and CDIC findings

TN 092 says the file-position pointer advances only when a sector passes the
file/channel mask, while trigger events can reach the CPU even from an
unselected channel. The May 1994 Green Book makes the exact rule explicit:
EOF and trigger require file selection, while EOR requires both file and
channel selection at the CD driver interface. This initially suggested that
the CDIC `time` register might need autonomous updates, but cdi220b BIOS
disassembly rules that out:

- `cdapdriv` converts the requested logical position to BCD and writes it before
  command `0x2A`;
- its delivered-sector callback updates the path descriptor's current file
  position only for selected sectors;
- its filter routine at ROM `0x2A804` receives the CDIC event sector, then
  preserves EOF/TRIG but clears EOR and the data-type flags when the channel
  was not selected; and
- command `0x2E` follows channel/configuration-register writes, so it is a
  configuration commit, not a “read current position” operation.

Therefore the HLE design—live register writes, a no-op `0x2E`, CDIC event
sectors bypassing the channel mask but not the file mask, BIOS-level EOR
masking, and no autonomous CDIC time-register advance—is intentional. Unit
tests lock the low-level selection semantics down. Seek latency should be
modeled separately from the 75-sector/s delivery clock: TN 076 gives a Green
Book worst-case allowance ranging from about one second locally to three
seconds across a disc, while TN 095 reports poorly laid-out authored projects
taking even longer.

## Video findings

The MCD212 data sheet gives the useful timing envelope: PAL has 312 total/280
active lines at a nominal 50 Hz and NTSC has 262 total/240 active lines at a
nominal 60 Hz. DCA storage always has a 64-byte line stride, but the retrace
fetch budget is only 32 bytes with CF clear and 64 bytes with CF set. The
renderer now enforces that conditional budget and has a synthetic regression
for the automatic stop/64-byte stride distinction. The channel-1 DE bit is the
master ICA/DCA gate for both paths; within an enabled display, DCA also requires
both IC and DC. Both rules are now enforced by synthetic tests. The first DCA
slot is fetched immediately after its ICA program, and subsequent slots are
advanced after visible lines so their state applies to the intended line rather
than one line late. The PAL regression verifies that this consumes exactly 280
64-byte slots, not an extra end-of-frame slot. Before changing line timing, the
15 MHz CPU clock must be reconciled with the separate PAL and NTSC video
crystals documented in TN 076; forcing every time base to an exact nominal
rate would contradict TN 094.

ICA/DCA interrupt instructions always record IT1/IT2 status, but DI1/DI2 are
active-high propagation disables for the shared CPU interrupt pin. CSR2 status
reads clear IT1, IT2 and BE. Those pin/status distinctions are now modeled and
tested instead of asserting INT for every interrupt instruction.

TN 063 and TN 069 explain visible races that titles may expose: CPU changes to
two plane pointers are sequential rather than atomic, linked LCT execution can
begin immediately after the final FCT instruction, and differing plane-A/B FCT
lengths can shift the two DCP streams. Plane B's image coding method is governed
by plane A. These are higher-value compatibility targets than emulating obsolete
18x color/matte defects globally.

One data-sheet cleanup remains deliberately deferred. Table 5-8 says
non-interlaced ICA always starts at byte `$400`, while interlaced odd/even fields
use `$400`/`$404`. Applying that rule directly to the current simplified field
scheduler prevented both title regressions from leaving the player shell. The
old alternating entry behavior has therefore been retained until scan-mode,
half-line and field scheduling are modeled together; the failed experiment and
its identical shell hash (`beddc17a...`) make this a known dependency rather than
an overlooked rule.

## Audio and input findings

The current audio path produces decoded samples immediately and approximates
transfer progress, but it has no separately timed audio-processor buffer. TN 079
and TN 069 make this observable: completion/status refer to RAM-to-processor
transfer, and the last buffered sample can remain audible after completion. A
future model needs separate transfer and playback cursors, with buffer depth as
a machine characteristic. The MCD221 has two physical ADPCM sector buffers;
TN 069 likewise describes the Philips reference player as effectively two-sector,
while warning that software cannot assume the same depth on all players.

The independent priority rule is implemented now: an active memory sound map
suppresses real-time-file ADPCM without canceling the map, and starting a map
aborts CD-DA. Focused CDIC tests protect both cases. Applications should still
derive A/V synchronization from disc position or triggers rather than sound-map
status.

The 605/615 hardware documents and TN 076 describe how connected devices turn
into `/ptr` and `/pt2` CSD entries. A port-1 device is handled through SLAVE;
port 2 uses the 68070 UART, and the remote can be the default when no external
device is present. The present frontend-to-SLAVE pointer path is correct for the
consumer-player default, but full multi-device compatibility eventually needs
the UART path and machine-specific CSD enumeration rather than a second copy of
the same HLE pointer.

## Compatibility roadmap derived from the archive

### P0: protect the working Mono-I path

1. Keep headless title boot/gameplay regressions for CD Shoot and both Alien
   Gate layouts, including CD-i Ready label access at absolute frame 16.
2. Add sector-selection regressions for ordinary selected data, unselected
   data, cross-channel EOF/EOR/TRIG and file-mask rejection. Add a BIOS-level
   trace assertion that `cdapdriv` clears cross-channel EOR before updating the
   play status.
3. Continue the DCP audit with concurrent two-plane FCT/LCT timing and complete
   scan-mode/field scheduling. First-LCT timing, DE gating and interrupt
   propagation now have synthetic control-program tests.
4. Keep seek/spin-up latency independent of continuous 75-sector/s delivery.

### P1: model behavior titles can observe

1. Split sound-map transfer completion from audible playback and add a bounded
   audio-processor FIFO model.
2. Add a deterministic compatibility-test mode for seek delays within the
   documented 1–3 second envelope; keep fast mode as a user option if desired.
3. Represent input ports/CSD selection explicitly when adding UART keyboard or
   second-pointer support.
4. Exercise PAL, NTSC and non-exact system/video time bases independently.

### P2: broaden hardware coverage deliberately

1. Add 18x defects only behind an 18x machine profile and only with title
   evidence.
2. Keep 605/615/660 extensions outside the Mono-I device assumptions.
3. Treat Digital Video/FMV support as a distinct subsystem; the archive's
   DV notes describe early-board problems, not a shortcut to MPEG emulation.

## Next document pass

The next compatibility investigations should route through these sources:

1. `docs/mc68hc05c8rg.pdf` plus the service-manual CD/MMC schematics for exact
   HC05 timer, SPI and line behavior if SLAVE/SERVO LLE replaces an HLE path.
2. MCD212 sections 5–9 plus TN 063/069/089 for concurrent two-plane control,
   half-line/interlaced scheduling and the remaining ICA entry-address cleanup.
3. `docs/mcd221tsrev0.pdf`, TN 069, TN 079 and the CDIC-facing driver behavior
   for a separately timed, machine-profiled audio transfer/playback FIFO.
4. TN 088 and the Digital Video service material only when the base Mono-I
   timing path is stable enough to begin FMV work.
5. `techdocs/cdi605_techdoc.pdf` and `cdi615_techdoc.pdf` when adding later
   boards, keeping Mono-I assumptions out of common device code.
