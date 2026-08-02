# Philips CD-i specification research

Status date: 2026-07-30

This is the durable research ledger for the local Philips CD-i/ICDIA document
archive. It records claims that can constrain emulation behavior, their exact
source, the current implementation assessment, and the next test needed before
changing a device. It is not a collection of title-specific workarounds.

## Source corpus and reproducible OCR

Set `$CDI_REFERENCE_ROOT` to the location of the local source archive:

```sh
export CDI_REFERENCE_ROOT=/path/to/icdia-site-documents
```

It currently contains 187 PDF files among 422 files (about 2.1 GiB). Original
documents are external references and are not committed. Searchable text and a
SHA-256 manifest are generated under ignored `references/spec-text/`:

```sh
scripts/ocr-cdi-specs.sh \
  "$CDI_REFERENCE_ROOT" \
  references/spec-text
```

The script first uses embedded PDF text and falls back to 240 dpi Tesseract OCR
when text is absent or consists only of repeated digitizer watermarks. Its
`manifest.tsv` records source-relative path, page count, extraction method,
source hash, and text path. An interrupted run is safe to repeat: useful
sidecars are reused and the manifest is replaced only after a complete pass.

Evidence authority, highest first:

1. Philips Green Book and component technical specifications.
2. Philips technical notes and documented CD-RTOS behavior.
3. Philips service manuals and developer/authoring documentation.
4. Firmware disassembly and runtime traces.
5. Independent implementations and observations.

Historical technical-note errata apply only to the player and software
revisions named by the note unless later evidence generalizes them.

### Priority-source fingerprints

These hashes pin the exact local editions used for the first priority pass:

| Relative source | SHA-256 |
|---|---|
| `docs/cdi_may94_r2.pdf` | `a2e851a163b7abd2248f28f4713324f70e112d29ec03923dd8cf3d55950b4a7c` |
| `docs/mcd212rev0.pdf` | `fac8b26b1830bd7899245c6f0bd56d2c42991ac80a3960578ff16dc7b1bb5947` |
| `docs/scc68070.zip` (`scc68070_apr93.pdf`) | archive `d75ac767121f9a8890d220afba420530de3aa9961c28452e2e4de672a4daf8b8`; PDF `71fbb838b265693bd2b8374fdc73e25c88827f90c7c24c439862bd2990bc57ff` |
| `svcmanuals/cdi205.pdf` | `43121b5f0e21590f080de070c21ab51f940d2540f8952f5b6c51a283f98c3b42` |
| `svcmanuals/cdi220.pdf` | `792b3f89274451df69bbd5bbc8a876c9b52d1cd938a980f6f6386517d1138429` |
| `svcmanuals/cdi350.pdf` | `6678dd584d850c313354a4121524c20e91bb8667c3cf0a653fa590fdfb0e8015` |
| `svcmanuals/cdi360.pdf` | `aae4874927aa756df4603b71041f26f4a54438486b85f345c07523d25ca5a265` |
| `svcmanuals/cdi450.pdf` | `b35a82eb22d49daea571773c6f557bb6bd8a493affd4ff4e1e8c4d3e88d8acdb` |
| `svcmanuals/22er9141.pdf` | `d1e23fd7b28413644f9c09c23cde2197763e337d5989ed8149ea621c3700bb41` |
| `notes/technote073.2.pdf` | `578d9434841abda7cc399cdbfdf27236a813e298ef30572cdb1f70bf9912b7ea` |
| `notes/technote088.pdf` | `1c3d3c2b60612215bb309dcfcdbc07e385197de60a710c4c8540c5ddfb30dae1` |
| `notes/technote094.pdf` | `e380122afff8155900246547dafec70166408de2254270a48a07d9dc6d84b0d8` |
| `notes/technote098.pdf` | `6c36811e5064c792287987b04cb0c15065445c03012f6694118f609ea9218df9` |
| `notes/technote102.pdf` | `f6b87a3cfb904a74e92cdc2af64bf51c4fc662553f6494c879cbef40f01e91d6` |
| `docs_sw/vcd_on_cdi_41.pdf` | `8a83f9fcce52b5d5ffb71b1ef367758256c00bc64bc079612c84e4897f0fb008` |
| `docs/keyboards_1996.pdf` | `7164caf116f07c78ade8a4fa7d3ba2e272e8104ca79e361df551398f5bebcd8e` |
| `docs/pointing_devices.pdf` | `660cf0d8cabfaf37236b2c11b1c522d07a0e39d60ee5a0abf0e55f96479c64ee` |

## Compliance matrix

| Subsystem | Documented behavior | Primary evidence | Current assessment | Next device-level evidence |
|---|---|---|---|---|
| Global timing | Disc/audio, video field, and 10 ms system tick are asynchronous; continuous A/V normally follows the disc/audio clock | `notes/technote094.pdf`, printed pp. 1-5 | The nominal 75-sector CDIC cadence and SCC68070 section-6.2 instruction/bus timing are implemented. The first accurate-timing divergence proved to be the CDIC/VMPEG interrupt-chain contract rather than sector cadence or DMA0 completion. | Preserve the bounded multi-device timeline when changing scheduler granularity; add a synthetic within-instruction deadline test before event slicing. |
| SCC68070 bus errors | External `BERRN` enters vector 2 and saves a 17-word format-F frame. SSW identifies function code, read/write and transfer attributes; setting SSW.RR suppresses rerunning the failed cycle on long-frame `RTE` | `docs/scc68070.zip` → `scc68070_apr93.pdf`, printed pp. 18 and 21-22, §§5.9-5.10, Figures 14-16; Table 22 for RTE timing | Implemented for the two independently verified Mono-I absent-memory ranges, with exact frame/RR tests. Unknown board holes remain open bus. This removes most of the firmware's bytewise RAM-search cost without a blanket unmapped-page rule. | Complete a physical Mono-I address-response matrix across holes, widths, directions and function codes before expanding fault coverage. |
| Player clocks | PAL system/CPU clocks are 30/15 MHz; NTSC system clock is 30.2098 MHz, consistent with the MCD212's 30.2097 MHz timing tables | `svcmanuals/cdi205.pdf`, PDF pp. 66 and 96; `svcmanuals/cdi350.pdf`, video specifications; `docs/mcd212rev0.pdf`, Table 5-5 | The scheduler and devices currently use a global 30/15 MHz constant, while MCD212 derives integer line periods from exact 50/60 Hz. This is a confirmed model limitation, but changing it before event-interleaved scheduling could regress verified titles. | Add board-clock values and line/field-period tests, then reconstruct scheduling so CPU and devices share the selected crystal without instruction-boundary lumping. |
| CDFM/PCL | CIL advances to `PCL_Nxt`; a full buffer cannot be reused; PCL signal precedes PCB signal; MPEG uses circular one-sector PCL chains | Green Book R2 `docs/cdi_may94_r2.pdf`, VII.4.4.2-VII.4.4.3 and IX.3.3.3; `notes/technote098.pdf`, “PCL Handling by the MPEG Drivers” | DMA-boundary ownership tracing, bounded guest-write provenance, and a synthetic reuse test are implemented. Current The 7th Guest and Addams runs show no full-PCL overwrite. The lone Addams hash difference is an intentional guest SCR retime, not payload corruption. | Retain these diagnostics when investigating a current visible transport failure; do not alter the nominal 75-sector cadence without a new first-divergence trace. |
| CDIC reset state | CDIC register 2 at base + `$3FFA` is nonzero after reset. The service manuals consistently say `$C7FE` in the PCB test and `$D7FE` in the terminal test | `svcmanuals/cdi205.pdf`, `svcmanuals/cdi220.pdf`, `svcmanuals/cdi350.pdf`, and `svcmanuals/cdi360.pdf`, PCB test step 9 and terminal test step 09 | `Cdic::new` currently initializes the corresponding Z/audio-control register to zero, so a reset mismatch is confirmed. The repeated bit-12 difference strongly suggests test-path state rather than a one-off typo; its cause must not be guessed. | Trace each BIOS's first accesses, identify what the terminal test initializes before reading register 2, and map `$C7FE`/`$D7FE` fields against CDIC documentation before correcting the reset state. |
| CDIC disc audio | On Mono-I, the first selected XA sector reports DBUF low nibble 4 and places a complete post-sync image in `$2800`; the second uses `$3200`/5. The coding byte is at ADPCM-buffer offset 11 and its 2304-byte payload begins at offset 12. `test_xa_play` also observes the timestamp/mode header changing in the ordinary `$0000/$0a00` data buffers during selected XA delivery, so the audio route does not suppress that guest-visible header. CDDA and CD-fed XA reading do not start audible playback: software writes `AUDCTL $0800` after the first buffer/IRQ. AUDCTL bit 0 is set by an `$ff`-coded stop and cleared by its status read, not toggled on every read. | Independent Mono-I hardware captures and tests in `Slamy/CDIC_BlackBoxAnalyzer` revision `e861f76`, `doc/cdic_manual.md`, `src/test_xa_play.c`, `src/test_cdda_play.c`, and `src/test_audiomap.c` | Implemented as dual header/audio routing plus separate receipt and playback state. The MAME-derived direct `play_realtime_audio_sector` shortcut is removed; focused tests cover ordinary-header visibility, first/second ADPCM placement, no PCM before `$0800`, CDDA gating, and the one-shot stop latch. The corrected routing restores Hotel Mario's Player 1 transition while bounded Earth Command and Alien Gate runs retain the previously verified audio behavior. | Manually recheck Hotel Mario Player 1, CDDA/XA start-stop, Earth Command termination, and Alien Gate firing. Model SLAVE `$82/$83` plus AD7528 output gating separately; these CDIC traces do not establish automatic physical-mixer unmute. |
| MCD212 geometry | Compatibility mode masks fixed samples/lines; 525 monitor and 525 TV have different `ST` meanings | Green Book R2 V.4.8; `docs/mcd212rev0.pdf`, Tables 5-4 through 5-7 and §5.8 | CD-i 220 TV and 625 behavior are modeled. The core has no distinct 525-monitor player type, so monitor-mode semantics are not representable. | Table-driven 525-monitor tests before adding a monitor-class model; do not alter CD-i 220 TV output. |
| Pixel aspect | Measured Philips output uses pixel-height/width ratios 1.225 for 525 and 1.025 for 625 | `notes/technote093.pdf`, printed p. 6 | Implemented exactly as 49/40 and 41/40. NTSC can legitimately look taller than PAL, and regional assets may also differ. | Compare raw MPEG dimensions, live MCD251 window, and final presentation before classifying PAL/NTSC framing as a bug. |
| MCD212 cursor | Blink on/off units are 12 TV fields: 200 ms at 60 Hz and 240 ms at 50 Hz | `docs/mcd212rev0.pdf`, §7.6 and cursor-control register description | Implemented with explicit field counting. PAL and NTSC register-level tests prove the state changes on the twelfth field in both standards. | Retain the field-count test when changing display scheduling; do not derive blink from CPU-cycle or nominal-frame accumulators. |
| Pointer devices | Relative devices report changes; maneuvering devices report continuously while deflected and support at least 16 directions; X/Y coexist in one packet | `docs/pointing_devices.pdf`, protocol sections “Relative” and “Maneuvering” | Relative mouse and simultaneous X/Y are conceptually correct. A fixed 60 Hz HLE polling cadence is not yet justified by this document alone. | Trace SLAVE firmware packet timing before changing `POLL_INTERVAL`; test diagonal and simultaneous-button packets. |
| Keyboard | K-mode uses 1200 baud, 8 data bits, one stop bit and two-byte change packets; T-mode uses 7 data bits, two stop bits and four-byte packets; both report Shift/Caps/Supershift/Control and ISO-8859-1 key codes | `docs/keyboards_1996.pdf`, v0.92, pp. 1-5; `docs/keyboard_drivers.pdf` | Host-keyboard passthrough is not emulated. The documents provide enough device-level protocol to implement K-mode first without mapping keys directly into guest memory. | Add a serial-packet encoder and SLAVE/UART recognition tests for press, release, modifiers, ID request, and idle silence. |
| SLAVE/SERVO | For drive traffic the SLAVE MC68HC05 transparently forwards four-byte command/data messages to a second drive MC68HC05 over SPI; `A0..A5` report status/time/track/version/echo/errors | `svcmanuals/cdi205.pdf`, PDF pp. 79-81 | Firmware-derived `B0` boot/media HLE works, but the physical drive-side state machine and open/close/spin-up phases are incomplete. | Correlate the SERVO firmware's SPI traffic with documented `A0..A5`/`AB` packets and the SLAVE flags before extending live media changes. |
| DVC memory/hardware | 22ER9141 supplies MPEG-1 decode and 1 MiB extra system RAM; compressed data comes from the 68070 and decoded RGB/audio returns to the base case | `svcmanuals/22er9141.pdf`, §§4.3-4.5 | The architecture matches the current CDIC/main-RAM/DMA/VMPEG path and base-case video/audio composition. The service diagram also confirms separate audio, video, buffering, and DRAM sections. | Preserve this path in transport fixes; add an initialization test for advertised extension memory and both decoder drivers. |
| DVC interrupt chain | The FMV extension exposes `INTREQN`/`IACKN`; CDIC and extension requests share IN4 through the base-case daisy chain, while the service glossary marks SCC68070 IN5 unused | `svcmanuals/cdi220.pdf`, PDF p. 66 and signal glossary | Implemented with a latched IN4 owner and owner-routed programmed-vector acknowledge. A focused test first reproduced the incorrect IPL5 request; the corrected bounded 7th Guest trace restores PCL order and eliminates the decoder failure. | Retain simultaneous-request and no-preemption tests; verify later player/DVC models separately rather than assuming the CD-i 220 chain. |
| DVC/player variants | Later CD-i 450/550 service material names model-specific DVC hardware (`22ER9144` in the regional power table and built-in `22ER9956` for the 550) | `svcmanuals/cdi450.pdf`, model notice and technical specifications | This reinforces the existing M3 boundary: the 22ER9141 VMPEG implementation must not be assumed to describe every later player/DVC combination. | Inventory the named cartridges and board interfaces as separate future models before extending M3 behavior to CD-i 450/550. |
| DVC memory descriptors | DVC adds priority-`$81` system RAM and color-`$90` MPEG memory; applications are guaranteed at least 960 KiB contiguous extension RAM | `notes/technote101.pdf`, printed pp. 1-3 | Address maps exist, and VMPEG firmware exposes CSD material. Guest-visible descriptor/allocation behavior is not directly asserted. | Parse regenerated CSD and verify `/mv`, `/ma`, `RAM00`, `RAM01`, priorities, colors, and minimum contiguous allocation. |
| MPEG play buffers | Normal video and audio playback use separate circular one-sector PCL chains; the drivers reset `PCL_Cnt`/`PCL_Ctrl` after consumption | `notes/technote098.pdf`, “PCL Handling by the MPEG Drivers” | Read-only diagnostics now observe PCL fills/releases, bounded guest changes, and DMA hashes. The working comparator and VCD sample complete without overwrite. Addams rewrites only two SCR bytes before submitting the pack; its 647 skipped audio bytes are legal initial Layer-II frame synchronization. | Apply the same provenance path to a user-visible sustained-play failure; treat MP2 sync acquisition separately from malformed frames. |
| MPEG events | Events occur at presentation time; last-picture means the first field displaying the last picture, not parser receipt of sequence end | Green Book R2 IX.3.3.5-IX.3.3.7 | SCR/PTS anchors and delayed-reference handling exist, but event timing has incomplete synthetic coverage. | Add first/last picture, new-sequence, underflow, input-error, and old-PCL-release timing tests. |
| MPEG sequence end | Concatenated encodes may introduce extra EOS codes; real cartridges can lose rhythm after some EOS codes | `notes/technote102.pdf`, printed pp. 1-2 | This is a real hardware edge case, but the authoring utility's patching workaround must not become an emulator media hack. | Build a project-owned stream with repeated valid EOS/SOS boundaries and test decoder/transport resynchronization. |
| MPEG transitions | Pause/continue, rapid abort/restart, starting on EOS+SOS, sequence changes, and PCL flushing have documented edge cases | `notes/technote088.pdf`, printed pp. 1-6 and issue summary; Philips FMVDemo `SUN/SRC/APPL/play_control.c` and `multilingual.c`; CDIC BlackBoxAnalyzer `e861f76` | Synthetic transition rules and the deterministic five-play run pass. The native FMVDemo pause/continue gate now passes after modeling measured DBUF/stop transport behavior and programming VMPEG's separate PAL/NTSC display period at `$e040aa`; bounded Pause retries retain TN 088's documented `E$NotRdy` behavior. Manual GUI testing confirms that both video and audio resume. The multilingual scenario still isolates one MP2 boundary error after `ma_cntrl` changes the FMA stream in place. Reverse scan is separately recorded because transient black macroblocks may reflect GOP-entry/reference handling or authentic scan behavior. | Determine the correct Layer-II FIFO boundary for in-place stream selection and compare reverse scan with real VMPEG hardware before changing decoder concealment. Retain both local scripts as executable gates. |
| CDIC sound maps | `SM_Done` means the last sector reached the audio processor buffer, not that it became inaudible; buffered audio may continue. Mono-I captures additionally show that `$ff` coding clears AUDCTL bit 11, sets one-shot bit 0, and raises the final enabled ABUF interrupt, while an AUDCTL-reset abort leaves ABUF bit 15 visible without an IRQ. | `notes/technote079.pdf`, printed pp. 1-2; `Slamy/CDIC_BlackBoxAnalyzer` `src/test_audiomap.c`, `test_audiomap_play_stop` and `test_audiomap_play_abort` | Per-half refill, one-/two-sector transfer completion, `$ff` completion, interrupt-masked abort, replacement after transfer completion, and preservation of the queued PCM tail have device-level coverage. `$ff` now ends the active map on its final buffer event instead of one tick later or issuing a duplicate completion. Manual macOS revalidation confirms Earth Command audio termination, Alien Gate firing maps, Golf Tips menu clicks, and Hotel Mario progression. | Investigate exact hardware audio-processor queue depth only if synchronization requires it. If intermittent silence recurs, retain a bounded CDIC event trace before changing the lifecycle. |
| Initialization | Application entry does not guarantee configurable player state; titles must initialize scan, compatibility, cursor, pointer origin, attenuation, and relevant DCP state | `notes/technote057.1.pdf`, printed pp. 1-2 | Different title screens may legitimately retain or program different state. The emulator must not cosmetically normalize it. | Capture reset/entry register state and first title writes in display incidents. |
| NVRAM/timekeeper | The MK48T08B is memory-mapped; the player stores game results, shell settings, FTS data, and CSD in NVRAM; maximum usable space is 31 KiB | `svcmanuals/cdi205.pdf`, PDF pp. 20 and 96 | The Mono-I model exposes a 32 KiB MK48T08-style device with eight clock registers and persists it by board. On `019fb59`, the PAL/VMPEG player shell retained three entries sized 13.3%, 2.4%, and 2.0%; their rounded 17.7% sum agrees with the displayed 18% total, leaving approximately 82% free. This confirms plausible shell accounting for the observed state but does not independently measure the byte-capacity reservation. | Preserve the current physical model; add a synthetic near-capacity filesystem test if exact reservation behavior becomes compatibility-relevant. |
| Disc addressing | Logical block address differs from Q absolute time by 150 frames; physical calibration can add a small disc-specific constant | `notes/technote066.pdf`, printed pp. 1-3 | The 150-frame relationship is implemented. Physical calibration offset is unnecessary for ordinary image-file reads unless hardware evidence requires it. | Add only if a subcode-sensitive title diverges from a hardware trace. |
| Player control keys | `/pck` Play/Stop/Pause/Next/Previous/Search keys are an optional extension, separate from the base two-button pointer; Pause is code `$82` with distinct key-down/key-up events | `notes/technote073.2.pdf`, printed pp. 1-3 | Start defaults to the configurable host-level E-Di menu because `/pck` is not yet emulated; Guide/Home, L1+R1, and right-stick alternatives are available. The overlay pauses emulation only while visible. Start is never emitted as a third base-pointer button, and Select is unassigned. | Implement `/pck` as an optional advertised device with `KB_Read`/`KB_Rdy`/`KB_SSig` behavior and `ss_enable` gating. Then expose an explicit choice between native Start-to-Pause down/up and host-menu use; titles that do not open `/pck` naturally ignore the native events. |
| Video CD engine | The engine requires DVC, starts the first PSD item, owns its control bars, and shows the multilingual dirty-disc message on disc errors | `docs_sw/vcd_on_cdi_41.pdf`, Release 4.1, pp. 7-9 | A native dirty-disc screen is affirmative evidence of a CD-i engine transport/play failure, not proof that the image is bad. | Correlate the dialog with the first PCL/decoder error and the exact PSD/entry point. |
| Video CD filesystem | Engine files are `CDI/CDI_VCD.APP;1`, `CDI/CDI_IMAG.RTF;1`, and `CDI/CDI_TEXT.FNT;1`; the PVD application identifier launches `CDI/CDI_VCD.APP;1` | `docs_sw/vcd_on_cdi_41.pdf`, Release 4.1, p. 9 | `DiscInventory` can identify root `CDI` content. Media-panel work should preserve the on-disc application instead of bypassing it. | Add a synthetic ISO/CD-XA fixture with the documented paths and PVD identifier. |
| Photo CD | A compliant Photo CD is a CD-i Bridge disc with an on-disc CD-i application; version 3.x supports thumbnails, per-photo resolution state, playlists, Portfolio audio, interruptible loads, rotation, and zoom | `faq/cdifaq5.html`, §§5.9-5.9.2; external package `sw_app/photocd_on_cdi_32.zip` | The Photo CD crate already parses `PHOTO_CD/INFO.PCD`, images, and playlists and presents host controls while retaining the CD-i application path. Package strings corroborate `PHOTO_CD/IMAGES`, `OVERVIEW.PCD`, `PLAYLIST.PCD`, audio, and per-disc NVRAM use. | Add a payload-free `DiscInventory` Photo CD classification and synthetic Bridge/PVD test; compare host controls with the native application rather than assuming all versions expose identical features. |

## Detailed findings

### The three CD-i time bases

Philips TN 94 says the 75-sector disc rate, 50/60-field video rate, and nominal
10 ms system tick are asynchronous and unlocked. Disc and ADPCM/sound-map
processing are tightly coupled and are the most stable clock. Field timing is
best for smooth visual effects, while a menu may use the system tick. PCL-full,
file-position, Trigger/EOR, and `SM_Done` are the documented ways for software
to observe disc/audio progress.

Consequences:

- Do not “fix” CD Shoot animation speed by changing the physical disc rate.
- A correct scheduler must permit the clocks to drift relative to one another
  while preserving each device's own cadence.
- Long-running A/V tests need timestamps from all three domains.
- Pointer-register updates are described as typically occurring on multiples
  of the system tick, but this does not override observed SLAVE firmware
  behavior.

The CD-i 205 service manual supplies the physical-clock detail missing from
the application note: PAL uses a 30.0000 MHz system clock and 15.000 MHz CPU
clock, while NTSC uses 30.2098 MHz. MCD212 Table 5-5 independently specifies
the 30.2097 MHz 525-TV sample clock and 63.56 us line. E-Di currently has one
30/15 MHz scheduler clock and derives integer line cycles from exact 50/60 Hz,
so region switching does not yet switch the underlying virtual crystal. This
is a genuine architectural gap, but a direct constant replacement would
repeat the scheduling rabbit hole documented in
`docs/core-compatibility-audit.md`. Board clocks and event interleaving need
to be changed and tested together.

### CDFM/PCL ownership result

Green Book R2 specifies Form-2 video payloads of 2,324 bytes and Form-2 audio
payloads of 2,304 bytes (the final 20 audio bytes are ignored). `PCL_Cnt`
identifies the next destination offset. When a PCL fills, the CIL advances to
`PCL_Nxt`; attempting to reuse a still-full PCL is an overrun condition. If PCL
and PCB signals coincide, the PCL signal is delivered first.

The MPEG driver documentation adds the missing consumer side: CDFM feeds
one-sector circular PCL chains, while `/mv` and `/ma` clear `PCL_Cnt` and
`PCL_Ctrl` only after consuming a buffer. The DMA-boundary ownership trace now
models that contract and a synthetic ring proves it can detect reuse before
release.

The current comparison falsifies simple PCL reuse for this sample. The 7th
Guest completes 1,296 VMPEG packs with orderly fill/release events and zero
decoder errors. Addams Family Values USA records no overwrite risk. Bounded
guest-write provenance shows that its lone hash difference at `$22CD88`
changes exactly two MPEG pack-header SCR bytes before DMA2 submission; the
elementary audio is unchanged. The first valid Layer-II frame header is 647
payload bytes later, exactly matching the old “647 audio errors” count.
Beginning in the middle of an audio frame is legal synchronization, so the
decoder now reports those bytes separately from malformed frames. Two
150-million-instruction runs remain deterministic with 464 packs, 215 decoded
video frames, 45 decoded audio frames, zero stream errors, and 647 audio
resynchronization bytes. The earlier historical overwrite remains contextual
evidence for other incidents, not a reason to slow the nominal pump rate.

### MPEG end and transition behavior

Green Book full-motion events are display-timed. Parser receipt of EOS is not
equivalent to “last picture displayed,” especially with reordered pictures.
TN 88 further documents old B-pictures after abort, corrupt output from an
immediate freeze/continue, special handling when EOS and SOS share a sector,
the need to start an initial play at a system/sequence header, and exact
two-audio-PCL retention across pause. TN 102 records a real cartridge rhythm
loss after some redundant EOS codes.

These are separate failure modes. A generalized test set should distinguish:

1. parser recovery at EOS/SOS;
2. delayed-reference presentation and last-picture timing;
3. driver PCL release;
4. pause/continue retention;
5. abort/restart removal of stale B-pictures;
6. a true dimension or sequence-header change.

The M3.14 synthetic suite now covers items 1, 2, 4, and 5 directly. Its
project-owned 16x16 I/B/B/P stream also survives 128 consecutive EOS/SOS
transitions under varying input fragmentation. Device tests preserve queued
pictures across pause/continue, clear all stale picture state at decoder
reset, and delay the final picture/EOD/underflow signals until presentation.
A selected-stream test prevents an ignored PES stream from changing the
accepted payload or PTS, and an exact six-hour shared SCR/PTS mapping guards
the integer A/V timebase. The original implementation nevertheless
established separate audio and video anchors at their respective DMA arrival
times. Green Book IX.4.6.2.2 instead says the play which first receives disc
data supplies synchronization information to the other. A device regression
now delays the second DMA by one second and proves it still uses the first
stream's clock. The subsequent first-PES timestamp correction has now passed
manual synchronization checks in Philips FMVDemo and three Video CD titles;
driver-level PCL release and title-level long-run timing remain the next
evidence boundary.

The headless acceptance runner now supplies that title-level timing boundary.
Bounded diagnostic milestones retain cumulative DVC counters and the 45 kHz
DCLK only when play, pause, first/last-picture, program-end, or audio
start/stop state changes. A payload-free postprocessor groups milestone-raster
hashes and counter deltas by play. Two exact 1.1-billion-instruction runs
produced byte-identical diagnostics and summaries with five play epochs and
zero decoder errors. Independent audio/video throughput estimates are retained
as evidence but are deliberately not subtracted into a purported sync result:
some epochs continue after one stream has ended or external video is hidden.
A timestamp- or presentation-overlap-based oracle is still required for
long-run perceptual A/V drift.

That oracle now also records the first SCR anchor, selected audio/video PTS,
scheduled DCLK deadlines, decoder-output clocks, and the first core audio
release/video latch. A deterministic Addams Family Values USA comparison
proved that the former demux used the final PTS in each completed DMA batch as
the timestamp of the first queued output. It thereby introduced a 304 ms
audio-leading-picture startup phase. The on-disc MPEG track starts audio and
video at equal PTS 68,999; preserving the first selected PES timestamp removes
408 ms of artificial phase without changing the 45 kHz clock or frontend
buffering. This corrects startup provenance only. Continued drift or a
remaining fixed offset must be tested by associating later PTS marks with
their decoded access units.

### Display and region observations

TN 93 calculates theoretical pixel-height/width ratios of about 1.19 for 525
and 1.05 for 625, then reports measured Philips-player values of 1.225 and
1.025. The same source explicitly warns that a bitmap can look vertically
stretched on a 525 player or compacted on a 625 player. Dedicated regional
assets can differ, and cross-standard presentation may introduce about 14%
distortion.

Therefore the existing PAL/NTSC menu and The 7th Guest comparison incidents
remain valid questions, but the screenshots alone do not prove an emulator
bug. For each comparison, preserve:

- exact disc fingerprint and player firmware;
- raw MPEG sequence dimensions/aspect/frame rate when applicable;
- live MCD212/MCD251 window and standard;
- hardware aperture;
- frontend pixel-aspect correction.

Green Book V.4.8 also distinguishes a 525-line monitor from a 525-line TV:
their horizontal Compatibility Mode meanings are reversed. This is a future
player-model boundary, not a reason to alter the current CD-i 220 TV.

The MCD212 cursor blink calculation is different: §7.6 is explicit. The former
accumulator produced 12 fields per unit at 50 Hz but only 10 at 60 Hz. The
implementation now counts fields explicitly, with a register-level regression
covering both standards.

### Video CD behavior and controls

The Philips Release 4.1 engine document explains several observations:

- without DVC it displays a startup message in English, French, and Japanese;
- on a disc error it displays the multilingual dirty-disc warning;
- it starts the first item in the Video CD 2.0 PSD;
- action button 1 toggles its native control bars;
- pause, step, slow, previous, next, stop, options, and position adjustment
  are implemented by the on-disc engine;
- the engine attempts to center the DVC window but intentionally offers a
  television-position adjustment screen.

Host VCD controls should therefore complement the on-disc CD-i application,
not replace or directly decode around it. A future VCD panel should translate
explicit host actions through a documented player/peripheral boundary.

### Keyboard implementation boundary

The 1996 keyboard specification documents two serial modes. K-mode matches the
earlier keyboard protocol: 1200 baud, 8 data bits, one stop bit, no parity,
with a two-byte packet whenever key or modifier state changes. T-mode uses
1200 baud, 7 data bits, two stop bits, no parity, and a four-byte packet.
Neither sends packets while state is unchanged. Both carry Shift, Caps Lock,
Supershift, Control, extension bits, and an ISO-8859-1 key code; an RTS-driven
recognition sequence requests the one-byte device ID.

This supports the TODO's keyboard feature as an optional emulated peripheral.
The generalized implementation belongs at the input-device serial boundary,
not as frontend-only injection into title memory. K-mode should be the first
fixture because it matches the established two-byte protocol.

### Photo CD application variants

The local site documentation describes Photo CD as a CD-i Bridge format whose
compliant discs include a CD-i playback application. The application version
matters:

- 1.x supplies thumbnails, ordering, fixed zoom, and rotation;
- 2.3 adds separate high/low resolution display and persists that choice in
  player NVRAM;
- 3.1 adds Portfolio playlists/audio, interruptible image loads, wipe
  transitions, and per-photo resolution state;
- 3.2 improves thumbnail performance;
- rare 3.2.1/3.3 builds add variable zoom.

The external Philips 3.2 package
`sw_app/photocd_on_cdi_32.zip` has SHA-256
`6672b37a7b4762c73008722059b5e7d68087018d8f356b0464af26c8a7a15e5c`.
Its module strings corroborate `PHOTO_CD/INFO.PCD;1`,
`PHOTO_CD/OVERVIEW.PCD;1`, `PHOTO_CD/IMAGES`, `PLAYLIST.PCD`, Portfolio audio,
PAL/NTSC QHY helpers, and `/nvr/PCD_SETTINGS*.nvri`. This package is a
reference artifact, not a file to incorporate into E-Di.

The existing host Photo CD panel should remain a complementary viewer. It
must not prevent the disc's own CD-i application from handling playlists,
audio, or version-specific interaction.

## Prioritized test queue

No emulation behavior changed during this research pass. Tests should precede
the corresponding corrections:

1. **VCD audio divergence (complete):** bounded provenance proves the guest
   intentionally retimes two SCR bytes and the 647-byte prefix is legal MP2
   frame-sync acquisition. No transport correction is warranted.
2. **CDIC reset register:** BIOS trace plus a focused nonzero `$3FFA` reset
   test after resolving the documented `$C7FE` versus `$D7FE` difference and
   mapping the bit fields.
3. **MCD212 cursor blink (complete):** PAL and NTSC register-level tests now
   prove one on/off unit lasts twelve fields.
4. **Three-clock timeline:** expose bounded disc, field, tick, audio-buffer,
   PCL, and pointer-update events on one diagnostic clock.
5. **MPEG transition suite:** project-owned EOS/SOS, delayed B-picture,
   last-picture, pause/continue, and abort/restart fixtures.
6. **DVC CSD/memory:** verify `/mv`, `/ma`, extension-RAM descriptors,
   priority/color, and contiguous allocation contract after boot.
7. **Video CD filesystem/PSD:** synthetic CD-XA image with the Philips engine
   path conventions and entry-point metadata.
8. **525 monitor boundary:** table-driven geometry tests only when a monitor
   player model is introduced.
9. **Sound-map completion:** separate transfer-done from audible completion
   and test stop/replacement tails.
10. **Keyboard peripheral:** K-mode ID, press/release, modifier, and idle-silence
   packet tests before exposing host passthrough.
11. **Photo CD Bridge inventory:** classify the documented filesystem and
    application path without extracting image payloads; preserve native
    application behavior when host controls are used.

Research remains open while the full archive OCR and lower-priority
player/service-manual scan continue. New findings should extend the compliance
matrix with a source and a falsifying test; they should not directly become
compatibility constants.
