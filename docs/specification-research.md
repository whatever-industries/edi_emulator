# Philips CD-i specification research

Status date: 2026-08-02

This is the durable research ledger for the local Philips CD-i/ICDIA document
archive. It records claims that can constrain emulation behavior, their exact
source, the current implementation assessment, and the next test needed before
changing a device. It is not a collection of title-specific workarounds.

## Source corpus and reproducible OCR

The OCR script defaults to the current local source archive. Set
`$CDI_REFERENCE_ROOT` only when the mirror has moved or on another checkout:

```sh
export CDI_REFERENCE_ROOT=/path/to/icdia-site-documents
```

The current mirror contains 185 PDF files (about 2.1 GiB). Original
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
| `docs/mcd251ts.pdf` | `fbf214d5c167459aad69d9e19cf1973358b689203c69e68eb3a09058667d8ba2` |
| Supplemental `CD-I full-motion video encoding on a parallel computer.pdf` | `290dd3c3967e09643fa245022dba35f2e045e0bbb191acbdfbb2cc4ffe26f437` |
| `docs/scc68070.zip` (`scc68070_apr93.pdf`) | archive `d75ac767121f9a8890d220afba420530de3aa9961c28452e2e4de672a4daf8b8`; PDF `71fbb838b265693bd2b8374fdc73e25c88827f90c7c24c439862bd2990bc57ff` |
| `svcmanuals/cdi205.pdf` | `43121b5f0e21590f080de070c21ab51f940d2540f8952f5b6c51a283f98c3b42` |
| `svcmanuals/cdi220.pdf` | `792b3f89274451df69bbd5bbc8a876c9b52d1cd938a980f6f6386517d1138429` |
| `svcmanuals/cdi350.pdf` | `6678dd584d850c313354a4121524c20e91bb8667c3cf0a653fa590fdfb0e8015` |
| `svcmanuals/cdi360.pdf` | `aae4874927aa756df4603b71041f26f4a54438486b85f345c07523d25ca5a265` |
| `svcmanuals/cdi450.pdf` | `b35a82eb22d49daea571773c6f557bb6bd8a493affd4ff4e1e8c4d3e88d8acdb` |
| `svcmanuals/22er9141.pdf` | `d1e23fd7b28413644f9c09c23cde2197763e337d5989ed8149ea621c3700bb41` |
| `notes/techindex.pdf` | `dcd73dfb2e413d06f376dc60f47ee2e7142477e3f7dabc3a795baea4984cdaf3` |
| `notes/technote022.pdf` | `6dfa381841df7abd70c0c54c680c7650d905abcbcace8eb02e35b3a8b2a83d38` |
| `notes/technote034.pdf` | `269c02c60b03b43d5e72622f67f629db22cd3f0d293fe5c0ce0e137ec46e207c` |
| `notes/technote039.pdf` | `e8dc3c142ae8f7fa8350e0ace070ef1c58e0bba09ee743a6124d38a9b7d0d503` |
| `notes/technote042.pdf` | `56f21ea860e18c5660857c0072e1d95ccc87736af7350be86f4349d505612277` |
| `notes/technote046.pdf` | `d3033bcfe0bdfa9ae0415ebe7cbdaa6533cec0fb55d601a76dbaa96ca9ab4c16` |
| `notes/technote048.pdf` | `fce7ee3f550fec0ea75dc192535f073418dbdb4836348d99d2dcf9312a3ec3a7` |
| `notes/technote049.pdf` | `4c6cf66dde11b64fcfa1cb0c8924b753cae55322b03d6454138ce4dc11fc8cef` |
| `notes/technote053.pdf` | `21ddae134d5531fc50b385d00f05bbc7a2b4dbfcda4bc63c010d5121a422a31f` |
| `notes/technote054.pdf` | `a95ef94ba5f95d0da803a126e6c85af6b4c2558d7682b1fbe0fb9bea4abf3231` |
| `notes/technote058.pdf` | `ee982800044a7ed2edb8dcd7cb5a2f2ba199099350c472e119ee5bc2e0fb11ac` |
| `notes/technote062.pdf` | `68c1dff4137cb88a4c5fa8c9bda741c4dce3393b394378bed0a0d8ec2eb96067` |
| `notes/technote063.pdf` | `3a2054a47f1faa4de9f7f7fb926023eb2a8dc7f3c66fd33c8267a4061f58e547` |
| `notes/technote068.pdf` | `45cd9e7e57159d153ae2d379acd965479b937f72d2a4dbb73fe43d17a353d694` |
| `notes/technote069.pdf` | `3a779194ad787c094766589d4d4810fe45191660621745b207977822a69e5ef4` |
| `notes/technote073.2.pdf` | `578d9434841abda7cc399cdbfdf27236a813e298ef30572cdb1f70bf9912b7ea` |
| `notes/technote076.pdf` | `421ce068a95362f516c6d9f0d6271d8460dbf4938b45f4c6ae2ca43526ecdc47` |
| `notes/technote085.1.pdf` | `69a1a8ab5d0d427c6ec64b58e6a875ef121a7c00d908dd5a52be178f15c15c84` |
| `notes/technote086.pdf` | `9b7201b5a2f232f86097d0296139b741a0b51088d39844ed690af8ca40c5bb6e` |
| `notes/technote087.pdf` | `acebbae0208769d6b11904682cb1005b692294aeac3e498af9d24ec109d0311a` |
| `notes/technote088.pdf` | `1c3d3c2b60612215bb309dcfcdbc07e385197de60a710c4c8540c5ddfb30dae1` |
| `notes/technote089.pdf` | `60280e2240efdbd6e0e8a268edb08a1d5c4c1e19cd648a02a5962cdf5740ff37` |
| `notes/technote090.pdf` | `b36e09b49c645063ed049a98a8f26af71e1a98adaa2773d0ca169f42e3edbff7` |
| `notes/technote092.pdf` | `65bebf71041f2fac3ce36090e7003c61c330ad5259635eb77d6b5e9ea937348b` |
| `notes/technote093.pdf` | `0b0059f45a72e8c3efe8bccacba6661fd8939675ae81440ff4621b9278bd8a42` |
| `notes/technote094.pdf` | `e380122afff8155900246547dafec70166408de2254270a48a07d9dc6d84b0d8` |
| `notes/technote096.pdf` | `885f43a76405aefa5916136cb29ffced4989714cc48f791835dda4deed08fe20` |
| `notes/technote097.pdf` | `005245d22b22ec25b0a3d4b9508f3cd5fd2ebb90f9c2a6cc32d20a564b11f53b` |
| `notes/technote098.pdf` | `6c36811e5064c792287987b04cb0c15065445c03012f6694118f609ea9218df9` |
| `notes/technote099.pdf` | `ffce18f16cbf18458ae352cf807d9218c80e20cdbd0fa86b1a30323b9e80cdd8` |
| `notes/technote100.pdf` | `27397f2d9b264d78f872dd0793af1e2ac3918e337e9b328b0c8b0e659eed384f` |
| `notes/technote102.pdf` | `f6b87a3cfb904a74e92cdc2af64bf51c4fc662553f6494c879cbef40f01e91d6` |
| `notes/technote103.pdf` | `6d919a4e47927307a8ef979675d80bb68c6951597c89eeccb5c988f033b6d6bc` |
| `notes/technote104.pdf` | `90916431822b7205b97c2a46b221f8a4ae3085aeed9f456c45082f4175409ab3` |
| `notes/technote105.pdf` | `e21baf0473879aa2f3237a8737c5d684bba58c7279eb0520d83061bda32acc78` |
| `authoring/master.pdf` | `4f3c7c4a737fea8ba5a8cf20d75cec776a906e264b0489e77fbd6c5fa1b282fa` |
| `docs_sw/vcd_on_cdi_41.pdf` | `8a83f9fcce52b5d5ffb71b1ef367758256c00bc64bc079612c84e4897f0fb008` |
| `docs/keyboards_1996.pdf` | `7164caf116f07c78ade8a4fa7d3ba2e272e8104ca79e361df551398f5bebcd8e` |
| `docs/pointing_devices.pdf` | `660cf0d8cabfaf37236b2c11b1c522d07a0e39d60ee5a0abf0e55f96479c64ee` |

## Compliance matrix

| Subsystem | Documented behavior | Primary evidence | Current assessment | Next device-level evidence |
|---|---|---|---|---|
| Global timing | Disc/audio, video field, and 10 ms system tick are asynchronous; continuous A/V normally follows the disc/audio clock. The tick may be fixed anywhere from 99.8 to 100.2 Hz on a given player; it does not fluctuate inside that range. | `notes/technote085.1.pdf`, printed p. 2; `notes/technote094.pdf`, printed pp. 1-5; `notes/technote100.pdf`, printed p. 7 | The nominal 75-sector CDIC cadence, exact 100 Hz tick, and SCC68070 section-6.2 instruction/bus timing are implemented. The first accurate-timing divergence proved to be the CDIC/VMPEG interrupt-chain contract rather than sector cadence or DMA0 completion. The allowed off-nominal tick is not modeled. | Preserve the bounded multi-device timeline when changing scheduler granularity; add a synthetic within-instruction deadline test and fixed 99.8/100.2 Hz tick variants before event slicing. |
| SCC68070 bus errors | External `BERRN` enters vector 2 and saves a 17-word format-F frame. SSW identifies function code, read/write and transfer attributes; setting SSW.RR suppresses rerunning the failed cycle on long-frame `RTE` | `docs/scc68070.zip` → `scc68070_apr93.pdf`, printed pp. 18 and 21-22, §§5.9-5.10, Figures 14-16; Table 22 for RTE timing | Implemented for the two independently verified Mono-I absent-memory ranges, with exact frame/RR tests. Unknown board holes remain open bus. This removes most of the firmware's bytewise RAM-search cost without a blanket unmapped-page rule. | Complete a physical Mono-I address-response matrix across holes, widths, directions and function codes before expanding fault coverage. |
| Player clocks | PAL system/CPU clocks are 30/15 MHz; NTSC system clock is 30.2098 MHz, consistent with the MCD212's 30.2097 MHz timing tables | `svcmanuals/cdi205.pdf`, PDF pp. 66 and 96; `svcmanuals/cdi350.pdf`, video specifications; `docs/mcd212rev0.pdf`, Table 5-5 | The scheduler and devices currently use a global 30/15 MHz constant, while MCD212 derives integer line periods from exact 50/60 Hz. This is a confirmed model limitation, but changing it before event-interleaved scheduling could regress verified titles. | Add board-clock values and line/field-period tests, then reconstruct scheduling so CPU and devices share the selected crystal without instruction-boundary lumping. |
| CDFM/PCL | CIL advances to `PCL_Nxt`; a full buffer cannot be reused; PCL signal precedes PCB signal; MPEG uses circular one-sector PCL chains. Balboa independently demonstrates circular per-channel/type buffer lists filled asynchronously in whole 2324-byte Form 2 sectors. | Green Book R2 `docs/cdi_may94_r2.pdf`, VII.4.4.2-VII.4.4.3 and IX.3.3.3; `notes/technote098.pdf`, “PCL Handling by the MPEG Drivers”; `notes/technote099.pdf`, printed pp. 18 and 67-69 | DMA-boundary ownership tracing, bounded guest-write provenance, and a synthetic reuse test are implemented. Current The 7th Guest and Addams runs show no full-PCL overwrite. The lone Addams hash difference is an intentional guest SCR retime, not payload corruption. | Retain these diagnostics when investigating a current visible transport failure; do not alter the nominal 75-sector cadence without a new first-divergence trace. |
| CDFM error reporting and correction | Non-real-time Form 1 sectors may legally occur inside a real-time file and be delivered asynchronously with ECC, but software correction can interrupt subsequent real-time delivery. Form 2 may carry EDC without ECC. PCL `PL_ERR.Err_Res` can report byte or word resolution, requiring worst-case application buffers of 294 bytes for Form 2 and 256 for Form 1. | `notes/technote054.pdf`, printed pp. 2-5; `notes/technote058.pdf`, printed pp. 1-2 and 4 | Clean image-backed sectors preserve form/real-time/EDC metadata but expose no injected read-error path, correction latency, or guest `PL_ERR` population. Native dirty-disc dialogs therefore cannot yet be tied to a physical-error resolution. | Add project-owned correctable and uncorrectable Form 1/Form 2 sector fixtures, byte/word `PL_ERR` variants, and a timeline proving that software ECC can delay a real-time stream without changing the nominal clean-sector cadence. |
| CDFM seek contract | `I$Seek` changes only the logical file-position pointer. Disc-reading calls perform an implicit physical seek; `SS_Seek` additionally repositions the head immediately and is useful only when that movement can overlap other presentation. A normal `SS_Seek` followed by a read can therefore move the head twice. | `notes/technote090.pdf`, printed pp. 1-2 | The image-backed drive has logical position and command latency, but does not model a physical head or distance-dependent implicit seek. This is a compatibility timing gap, not a reason to delay every host read. | Add a synthetic SLAVE/CDIC timeline for logical seek, pre-positioned asynchronous seek, near implicit seek, and cross-disc seek before modeling head position or Green Book 1-to-3-second limits. |
| CDIC reset state | CDIC register 2 at base + `$3FFA` is nonzero after reset. The service manuals consistently say `$C7FE` in the PCB test and `$D7FE` in the terminal test | `svcmanuals/cdi205.pdf`, `svcmanuals/cdi220.pdf`, `svcmanuals/cdi350.pdf`, and `svcmanuals/cdi360.pdf`, PCB test step 9 and terminal test step 09 | `Cdic::new` currently initializes the corresponding Z/audio-control register to zero, so a reset mismatch is confirmed. The repeated bit-12 difference strongly suggests test-path state rather than a one-off typo; its cause must not be guessed. | Trace each BIOS's first accesses, identify what the terminal test initializes before reading register 2, and map `$C7FE`/`$D7FE` fields against CDIC documentation before correcting the reset state. |
| CDIC disc audio | Direct real-time audio delivery is autonomous and does not consume the CPU bus. Its file-position pointer advances only when an ADPCM sector passes the selection mask, whereas any trigger can signal even from a non-selected channel. On Mono-I, the first selected XA sector reports DBUF low nibble 4 and places a complete post-sync image in `$2800`; the second uses `$3200`/5. CDDA and CD-fed XA still require guest `AUDCTL $0800` before audible playback. | `notes/technote092.pdf`, printed pp. 1-2; independent Mono-I hardware captures in `Slamy/CDIC_BlackBoxAnalyzer` revision `e861f76`, `doc/cdic_manual.md`, `src/test_xa_play.c`, `src/test_cdda_play.c`, and `src/test_audiomap.c` | Implemented as dual header/audio routing plus separate receipt and playback state. Focused tests cover ordinary-header visibility, first/second ADPCM placement, no PCM before `$0800`, CDDA gating, and the one-shot stop latch. Selected-sector file-position and cross-channel trigger semantics are not yet represented explicitly. | Preserve the hardware-verified buffer route. Add device tests for selected-sector file-position updates and non-selected-channel triggers before extending CDIC/CDFM synchronization behavior. Model SLAVE `$82/$83` plus AD7528 output gating separately. |
| SLAVE analog audio output | The SLAVE serially programs two AD7528 devices for mute/unmute and the four-path stereo attenuation matrix. An external Hotel Mario score-screen SPI trace contains 55 four-byte-equal writes: ordinary updates are about 0.775-0.825 ms apart, a 0-to-255 ramp lasts 27.132 ms, and a 252-to-0 ramp lasts 24.031 ms; occasional 2.2-2.9 ms gaps are consistent with SLAVE workload. | Firmware command forms `$82`/`$83` and `$C0..$CF`; local external Saleae text export SHA-256 `0cd157158a3dd637fa3661e1840a04f799491b1da58cc82c3477335709ea2830` | The HLE parses the attenuation packet and applies a digital CDIC matrix, but does not model the serial DAC cadence, analog transfer function, mute latch, or a shared final boundary for VMPEG audio. The trace constrains timing only; repeated bytes do not prove the general command mapping. | Capture ch2 commands, SPI words, analog output, and source identity on one synchronized timeline. Establish code polarity and gain before changing audible output; then test whether CDIC and DVC both pass through the same boundary. |
| CDFM play termination | `PCB_Rec` counts EOR-delimited real-time records. `ss_abort` is the reliable termination path. Green Book permits a live zero to be recognized on the next selected sector, but TN 085.1 warns that some players wait for a sector selected for RAM delivery; its portable workaround clears `PCB_AChan` immediately before `PCB_Rec`, with the selected audio CIL entry either valid or zero. `PCB_Chan` selects processed channels while `PCB_AChan` separately routes selected audio directly to the audio processor. | Green Book R2 VII.2; `authoring/master.pdf`, printed pp. 3-64--3-65; `notes/technote085.1.pdf`, printed p. 6; `notes/technote099.pdf`, printed p. 69; Philips FPD805 `dev/basecase/bmp_nat/test/bumptest.c` and `dev/basecase/bmp_nat/code/src/bumpanim.c` | Read-only diagnostics snapshot PCB record/routing changes, and bounded harness patches now make deliberate guest-RAM changes auditable. The exact-FPD805 local gates cover natural three-record exhaustion, its native abort path, a live direct-audio `PCB_Rec = 0`, and TN 085.1's workaround with null and valid one-sector audio CIL entries. On the current 220 ROM all three live-clear cases wait for the same next selected audio-sector interrupt; the workaround cases have cycle-identical `PCB_Rec`, selected-sector, and route-clear milestones. FPD805 source also shows that its abort handler clears `PCB_AChan` before the later `ss_abort`, correcting the earlier attribution. No emulation behavior changed. | Retain this 220 gate. Do not generalize its permitted fast direct-audio result to other player models; add an alternate native driver/player fixture before modeling TN 085.1's slower compatibility phenotype. |
| MCD212 geometry and interlace | Compatibility mode masks fixed samples/lines; 525 monitor and 525 TV have different `ST` meanings. For cross-standard playback, authoring can center a 240-line 525 picture in the 280-line 625 aperture by adding 20 neutral scanlines above and below; the compatible 280-line format can fill the aperture. Base-case CD-i can display true interlaced odd/even drawmaps; high-contrast horizontal detail then exhibits authentic interline flicker. | Green Book R2 V.4.8; `docs/mcd212rev0.pdf`, Tables 5-4 through 5-7 and §5.8; `notes/technote042.pdf`, printed pp. 1-4; `notes/technote048.pdf`, printed pp. 1-2; `notes/technote099.pdf`, printed p. 17 | CD-i 220 TV and 625 behavior are modeled, and the core retains odd/even field rows. The core has no distinct 525-monitor player type. The live cursor is composited after the weave, but title-authored high-frequency detail can still comb or flicker. The border notes describe authored/manager policy, not permission to crop a title's final row. | Retain parity/field-weave tests. Add a project-owned interlaced high-contrast fixture before changing presentation filtering; do not turn authoring-side filtering or borders into an MCD212 device filter. |
| Composite-output boundary | At the 7.5 MHz normal-resolution dot rate, one- and two-pixel detail can acquire false hue and brightness variation after analog NTSC composite encoding even though the same digital RGB pixels remain correct. The effect applies to every CD-i coding method and is strongest in narrow high-contrast detail. | `notes/technote039.pdf`, printed pp. 4-5 | The core and raw presentation expose the digital raster, not a guessed manufacturer-specific composite encoder/decoder. This can make emulator output look cleaner than a consumer CRT without implying a device-decoder error. | Add an explicitly optional, separately tested composite-display simulation only if requested; never use this note to alter decoded pixels, crop an edge, or excuse a digital-raster mismatch. |
| MCD212 DCA control | DCA fetches occur during horizontal retrace, allocate 64 bytes per line, and use a DCP-linked line sequence; Table 5-10 specifies the 32/64-byte per-line fetch budget and Table 5-12 documents STOP and reload operations. On the 910, the actual first linked LCT executes immediately after its FCT, unlike the Green Book model. A changed LCT instruction becomes effective only when the display reads it, potentially up to one field later; a live 32-bit instruction can theoretically be observed as old/new 16-bit halves. | `docs/mcd212rev0.pdf`, §5.4.2 and Tables 5-10/5-12; `notes/technote069.pdf`, printed p. 8, §3.3; `notes/technote100.pdf`, printed pp. 4-9 and 28-31 | The core advances the 64-byte DCA slot between unmasked display lines and raises display interrupts when the corresponding ICA/DCA command is processed. It does not yet prove the exact visible-line phase of a DCA interrupt or a CPU-write/display-fetch collision inside one 32-bit instruction. A plausible field-wide STOP latch was tested against Addams Family Values USA native controls, immediately destroyed the valid control bar, and was reverted. | Add a bounded line timeline that writes the two 16-bit halves around a DCA fetch and records the interrupt edge, active slot, and raster. Do not infer a field-wide STOP latch or hide the final display row. |
| MCD212 mattes | Matte flags reset to false at the start of every scanline, while the eight matte-control registers retain their commands from line to line until the DCP reloads them. Registers operate as one ordered set of eight or two ordered sets of four. If both paths load the same register during one line's control phase, path A wins and path B is ignored. | `docs/mcd212rev0.pdf`, §5.4.4.12; Green Book R2 V.5.10.1-V.5.10.3; `notes/technote099.pdf`, printed pp. 70-72 | Tests prove the false line start, persistent register template, ordered one-/two-set comparisons, STOP, and shared-register path-A priority even when the two loads occupy different command slots in the same line. The old sequential model applied path A first and then incorrectly let path B overwrite it; paired ICA/DCA execution now suppresses only the conflicting path-B load. TN 099's loose end-of-field wording remains subordinate to the later Green Book. | Retain these tests. Add a focused raster only if combined matte-flag/image-contribution-factor opcodes become compatibility-relevant. |
| MCD212 DCP integrity | Every display line must remain controlled by a defined linked LCT. Missing or overwritten links let hardware consume undefined memory. Illegal opcode/mode combinations have undefined results; on a Philips 220, masked DYUV with four-bit resolution can turn both planes solid gray. | `notes/technote085.1.pdf`, printed pp. 4-5 | Legal DCP decoding and linked LCT traversal are tested. There is no explicit diagnostic for a missing final LCT link or illegal transient state, and undefined combinations must not be normalized into a portable title behavior. | Add diagnostics that identify the first undefined LCT fetch and illegal DCP combination without prescribing its raster. Preserve the title's two-write order and scan phase before attributing a transient to the device. |
| MCD212 RGB555 | CD-i IFF stores RGB555 as an upper-byte plane followed by a lower-byte plane, while `dm_write()` accepts interleaved 16-bit pixels and splits them into the two drawmap banks. Rectangular IFF lines are longword-padded; real-time files place each bank on a sector boundary. | `notes/technote022.pdf`, printed pp. 1-3 | The device renderer reads plane B as the low byte and plane A as the high byte; a focused asymmetric-byte raster test now protects that order. The note constrains authored storage and UCM input layout, not a second display-device conversion path. | Retain the raster test and keep planar on-disc IFF distinct from interleaved `dm_write()` buffers in future inventory/drawmap provenance. |
| MCD212 DYUV scanlines | DYUV uses the fixed 16-value delta table with modulo-256 addition. Programmed Y/U/V start values are reapplied at every scanline, making lines independent. Horizontal panning has a two-pixel minimum and requires an authored, line-specific recomputed start value; prepared DYUV blits must meet both background edges. | `notes/technote034.pdf`, printed pp. 1-2; `notes/technote086.pdf`, printed pp. 1-10 | The renderer uses the documented table and wrapping addition, and initializes Y/U/V from the programmed start for each `process_vsr()` line. Focused tests now force a 250+9 wrap and prove that a changed first-line accumulator cannot leak into the second line. Panning and fitted blits remain authoring operations. | Retain the device regressions; add an authored two-pixel-offset fixture only if horizontal DYUV panning becomes compatibility-relevant. |
| Base-case UVLO motion video | UVLO is a title-supplied software codec, not an MCD212 image mode or DVC format. A base-case title receives sector-sized frames through a circular PCL chain, decodes them on the 680x0 into ordinary DYUV drawmaps, alternates drawmaps, and reveals the decoded window with a matte. The sample treats a producer/consumer queue collision as a dropped frame. | `notes/technote053.pdf`, printed pp. 1-7 and sample program pp. 9-17 | E-Di supplies the documented CDIC/CDFM delivery, DYUV drawmap, DCA/matte, compatibility-mode, and CPU primitives, but has no project-owned UVLO real-time-file fixture proving the complete native software path or its timing. UVLO must not be routed through VMPEG or decoded as a new host-side image format. | Build or locate a redistributable UVLO sample and assert circular PCL ownership, frame-drop behavior under deliberate decoder pressure, drawmap alternation, matte bounds, and 384-pixel TV compatibility mode. Use the Engineering 6.0 disc only as optional local evidence if it contains a UVLO case. |
| Pixel aspect | TN 093 reports measured Philips output at 1.225 for 525 and 1.025 for 625. The later TN 104 instead calculates 1.230/1.017 and reports empirical 1.235 ± 0.003/1.019 ± 0.003; it separately gives White Book Video CD source-pixel ratios of 1.1069/0.9157 and says the DV decoder does not act on the MPEG aspect-ratio field. | `notes/technote093.pdf`, printed p. 6; `notes/technote104.pdf`, printed pp. 7-10 | The frontend uses 49/40 and 41/40 from TN 093. The later source creates a genuine calibration dispute of about one percent, while also confirming that asset preparation, decoder window geometry, and player-output correction are separate. | Measure a known geometry on physical Mono-I NTSC and PAL output, including the final analog/digital capture aperture, before changing presentation ratios. Keep Video CD source-pixel ratios out of the player-output correction. |
| MCD212 cursor | Blink on/off units are 12 TV fields: 200 ms at 60 Hz and 240 ms at 50 Hz | `docs/mcd212rev0.pdf`, §7.6 and cursor-control register description | Implemented with explicit field counting. PAL and NTSC register-level tests prove the state changes on the twelfth field in both standards. | Retain the field-count test when changing display scheduling; do not derive blink from CPU-cycle or nominal-frame accumulators. |
| Pointer devices | Relative devices report changes; maneuvering devices report continuously while deflected and support at least 16 directions; X/Y coexist in one packet | `docs/pointing_devices.pdf`, protocol sections “Relative” and “Maneuvering” | Relative mouse and simultaneous X/Y are conceptually correct. A fixed 60 Hz HLE polling cadence is not yet justified by this document alone. | Trace SLAVE firmware packet timing before changing `POLL_INTERVAL`; test diagonal and simultaneous-button packets. |
| Player CSD and device discovery | Titles must discover device names/types through the CSD rather than assume Philips names. The 605 CSD adds player-control and extension devices; `/ptr` and `/pt2` depend on attached pointing devices and port placement, with the remote assumed when no external device is present. | `notes/technote076.pdf`, printed pp. 6-7; `notes/technote085.1.pdf`, printed pp. 4 and addendum | Firmware-supplied CSD data is used by native software, and the consumer remote currently reaches the SLAVE pointer path. Machine-specific CSD contents, alternate input types, two-device ordering, and non-Philips device names are not asserted. | Parse the live CSD after boot with zero, one, and two devices. Verify type, ordering, `/ptr`/`/pt2`, optional `/pck`, and extension entries per player model before adding a second input port. |
| Keyboard | K-mode uses 1200 baud, 8 data bits, one stop bit and two-byte change packets; T-mode uses 7 data bits, two stop bits and four-byte packets; both report Shift/Caps/Supershift/Control and ISO-8859-1 key codes | `docs/keyboards_1996.pdf`, v0.92, pp. 1-5; `docs/keyboard_drivers.pdf` | Host-keyboard passthrough is not emulated. The documents provide enough device-level protocol to implement K-mode first without mapping keys directly into guest memory. | Add a serial-packet encoder and SLAVE/UART recognition tests for press, release, modifiers, ID request, and idle silence. |
| SLAVE/SERVO | For drive traffic the SLAVE MC68HC05 transparently forwards four-byte command/data messages to a second drive MC68HC05 over SPI; `A0..A5` report status/time/track/version/echo/errors | `svcmanuals/cdi205.pdf`, PDF pp. 79-81 | Firmware-derived `B0` boot/media HLE works, but the physical drive-side state machine and open/close/spin-up phases are incomplete. | Correlate the SERVO firmware's SPI traffic with documented `A0..A5`/`AB` packets and the SLAVE flags before extending live media changes. |
| DVC memory/hardware | 22ER9141 supplies MPEG-1 decode and 1 MiB extra system RAM; compressed data comes from the 68070 and decoded RGB/audio returns to the base case. Philips' system description independently requires CPU-bus data/control, a CD data clock, base-case pixel/HSYNC/VSYNC inputs, and pixel-by-pixel video selection. | `svcmanuals/22er9141.pdf`, §§4.3-4.5; `notes/technote097.pdf`, printed pp. 25-29; Sijstermans and van der Meer, “CD-I Full-Motion Video Encoding on a Parallel Computer,” printed pp. 81-91 (decoder sidebar p. 90) | The architecture matches the current CDIC/main-RAM/DMA/VMPEG path and base-case video/audio composition. The service diagram also confirms separate audio, video, buffering, and DRAM sections. | Preserve this path in transport fixes; add an initialization test for advertised extension memory and both decoder drivers. Keep the early encoder paper as architectural corroboration, not a later Green Book replacement. |
| DVC interrupt chain | The FMV extension exposes `INTREQN`/`IACKN`; CDIC and extension requests share IN4 through the base-case daisy chain, while the service glossary marks SCC68070 IN5 unused | `svcmanuals/cdi220.pdf`, PDF p. 66 and signal glossary | Implemented with a latched IN4 owner and owner-routed programmed-vector acknowledge. A focused test first reproduced the incorrect IPL5 request; the corrected bounded 7th Guest trace restores PCL order and eliminates the decoder failure. | Retain simultaneous-request and no-preemption tests; verify later player/DVC models separately rather than assuming the CD-i 220 chain. |
| DVC/player variants | Later CD-i 450/550 service material names model-specific DVC hardware (`22ER9144` in the regional power table and built-in `22ER9956` for the 550) | `svcmanuals/cdi450.pdf`, model notice and technical specifications | This reinforces the existing M3 boundary: the 22ER9141 VMPEG implementation must not be assumed to describe every later player/DVC combination. | Inventory the named cartridges and board interfaces as separate future models before extending M3 behavior to CD-i 450/550. |
| DVC memory descriptors | DVC adds priority-`$81` system RAM and color-`$90` MPEG memory; applications are guaranteed at least 960 KiB contiguous extension RAM | `notes/technote101.pdf`, printed pp. 1-3 | Address maps exist, and VMPEG firmware exposes CSD material. Guest-visible descriptor/allocation behavior is not directly asserted. | Parse regenerated CSD and verify `/mv`, `/ma`, `RAM00`, `RAM01`, priorities, colors, and minimum contiguous allocation. |
| MPEG play buffers | Normal video and audio playback use separate circular one-sector PCL chains; the drivers reset `PCL_Cnt`/`PCL_Ctrl` after consumption | `notes/technote098.pdf`, “PCL Handling by the MPEG Drivers” | Read-only diagnostics now observe PCL fills/releases, bounded guest changes, and DMA hashes. The working comparator and VCD sample complete without overwrite. Addams rewrites only two SCR bytes before submitting the pack; its 647 skipped audio bytes are legal initial Layer-II frame synchronization. | Apply the same provenance path to a user-visible sustained-play failure; treat MP2 sync acquisition separately from malformed frames. |
| MPEG events | Events occur at presentation time; last-picture means the first field displaying the last picture, not parser receipt of sequence end. On early cartridges, the PIC signal is raised only after the DV picture has begun displaying, so applications needing a synchronized base-plane matte predict the next picture and update an earlier LCT. | Green Book R2 IX.3.3.5-IX.3.3.7; `notes/technote103.pdf`, printed pp. 2-4 | SCR/PTS anchors, delayed-reference handling, and display-timed final-picture coverage exist. The precise PIC-within-field phase and base-plane line-interrupt relationship remain untested. | Add a display-field timeline test for PIC, next-picture latch, MCD212 line interrupt, and a triple-buffered matte update before supporting picture-synchronous overlays. |
| MPEG sequence end | Concatenated encodes may introduce extra EOS codes; real cartridges can lose rhythm after some EOS codes. Seamless body streams deliberately omit sequence/program end codes unless parameters change or the whole play ends; an intermediate program end prevents continuation. | `notes/technote102.pdf`, printed pp. 1-2; `notes/technote105.pdf`, printed pp. 4-7 and 25 | The project-owned repeated EOS/SOS transition suite passes, but this does not justify accepting malformed authored branches or stripping codes from media. | Add introductory/body seamless-branch fixtures that distinguish legal same-parameter continuation, parameter-changing EOS+header, intermediate program end, and final end. |
| MPEG transitions and branching | Pause/continue, rapid abort/restart, EOS+SOS, sequence changes, and PCL flushing have documented edge cases. Truly seamless branching keeps decoders running, preserves VBV/STD boundary state, uses closed GOP entry, keeps PCL ownership until time stamps are patched, and derives the patch delta from PTS rather than SCR. | `notes/technote088.pdf`, printed pp. 1-6; `notes/technote105.pdf`, printed pp. 1-12 and 19-25; Philips FMVDemo `SUN/SRC/APPL/play_control.c` and `multilingual.c` | Synthetic transition rules and native pause/continue and stream-selection gates pass. The current tests cover decoder continuity and selected-PES boundaries, but not a general authored introductory/body branch with matched VBV/STD states or PTS patching across repeated branches. | Retain the native gates and add project-owned TN 105 introductory/body and closed-GOP branch fixtures. Verify PTS-derived delta continuity, buffer states, complete audio-frame boundaries, and PCL release without adding media repair. |
| CDIC sound maps | `SM_Done` means the last sector reached the audio processor buffer, not that it became inaudible; buffered audio may continue. Sound maps take audible priority over a continuing direct real-time play, and the amount of queued audio is player-dependent. Mono-I captures additionally establish `$ff`/AUDCTL completion behavior. | `notes/technote079.pdf`, printed pp. 1-2; `notes/technote092.pdf`, printed pp. 2-3; `Slamy/CDIC_BlackBoxAnalyzer` `src/test_audiomap.c` | Per-half refill, transfer completion, `$ff` completion, interrupt-masked abort, replacement, direct-audio suppression, and preservation of the queued PCM tail have device-level coverage. The exact physical queue depth remains intentionally abstract. | Investigate exact hardware audio-processor queue depth only if synchronization requires it. Preserve separate transfer-done and audible-done state and do not derive queue depth from one player family. |
| Initialization | Application entry does not guarantee configurable player state; titles must initialize scan, compatibility, cursor, pointer origin, attenuation, and relevant DCP state | `notes/technote057.1.pdf`, printed pp. 1-2 | Different title screens may legitimately retain or program different state. The emulator must not cosmetically normalize it. | Capture reset/entry register state and first title writes in display incidents. |
| Base and extension memory | Base CD-i provides two 512 KiB colored banks. At application entry, OS structures may consume at most 32 KiB in either bank, leaving at least 480 KiB contiguous per plane. The Philips DVC dynamically inserts 1 MiB system RAM and 0.5 MiB priority-zero MPEG RAM into the live memory list rather than the static init-module list. | `notes/technote087.pdf`, printed pp. 1-3 and 6-9; `notes/technote096.pdf`, printed pp. 9-10 | The physical base/DVC RAM maps exist. Guest-visible CD-RTOS memory-list insertion, priorities, colors, and entry-time contiguity have not been asserted from a boot trace. | Capture the post-DVC `D_FreeMem` list and application-entry free-block map; verify two base colors, dynamic extension nodes, priority-zero decoder RAM, and 480 KiB base-case minima. |
| NVRAM/timekeeper | The MK48T08B is memory-mapped; TN 068 describes a base-case player as having approximately 8 KiB and documents the native NVRUI deletion/protection policy and deliberately rounded percentage display. The service manual also describes a 32 KiB CPU window and says one configuration permits 31 KiB, so capacity must remain a board/configuration property rather than a universal shell constant. | `notes/technote068.pdf`, printed pp. 1-4; `svcmanuals/cdi205.pdf`, PDF pp. 20 and 96 | The Mono-I core stores 8 KiB of MK48T08 SRAM, maps it on the upper byte lane through the configured 32 KiB window, reserves the final eight device offsets for clock registers, and persists it by board. On `019fb59`, the shell's three rounded entry percentages sum plausibly to its rounded total, but this does not identify the exact filesystem reservation or reconcile every service-manual configuration. | Preserve the current 8 KiB device until a model-specific address probe proves otherwise. Add a native near-capacity filesystem test for percentage rounding, protection, and deletion ordering if shell storage becomes compatibility-relevant. |
| Disc addressing | Logical block address differs from Q absolute time by 150 frames; physical calibration can add a small disc-specific constant | `notes/technote066.pdf`, printed pp. 1-3 | The 150-frame relationship is implemented. Physical calibration offset is unnecessary for ordinary image-file reads unless hardware evidence requires it. | Add only if a subcode-sensitive title diverges from a hardware trace. |
| Player control keys | `/pck` Play/Stop/Pause/Next/Previous/Search keys are an optional extension, separate from the base two-button pointer; Pause is code `$82` with distinct key-down/key-up events | `notes/technote073.2.pdf`, printed pp. 1-3 | Start defaults to the configurable host-level E-Di menu because `/pck` is not yet emulated; Guide/Home, L1+R1, and right-stick alternatives are available. The overlay pauses emulation only while visible. Start is never emitted as a third base-pointer button, and Select is unassigned. | Implement `/pck` as an optional advertised device with `KB_Read`/`KB_Rdy`/`KB_SSig` behavior and `ss_enable` gating. Then expose an explicit choice between native Start-to-Pause down/up and host-menu use; titles that do not open `/pck` naturally ignore the native events. |
| Video CD engine | The engine requires DVC, starts the first PSD item, owns its control bars, and shows the multilingual dirty-disc message on disc errors. Release 4.1 says the control bar initially appears at the bottom, but the application is configurable. | `docs_sw/vcd_on_cdi_41.pdf`, Release 4.1, printed pp. 5-7 | A native dirty-disc screen is affirmative evidence of a CD-i engine transport/play failure, not proof that the image is bad. The documented configurability means a clean, flush bar on one title does not establish a different Video CD standard or a universal compositor boundary. | Correlate the dialog with the first PCL/decoder error and the exact PSD/entry point. Fingerprint the native engine/application and configuration before treating control-bar geometry as a cross-title invariant. |
| Video CD filesystem and PSD | Philips' reference engine files are `CDI/CDI_VCD.APP;1`, `CDI/CDI_IMAG.RTF;1`, and `CDI/CDI_TEXT.FNT;1`; the PVD application identifier launches `CDI/CDI_VCD.APP;1`. Authored discs may instead use the standard `VCD/INFO.VCD`, `VCD/ENTRIES.VCD`, and `MPEGAV/AVSEQ*.DAT` structure with another native CD-i entry point. White Book 2.0 defines entry addresses plus LOT-indexed selection, play, and end lists whose offsets use the multiplier in `INFO.VCD`. | Video CD 2.0, III.2.5 and VI.1-VI.6; `docs_sw/vcd_on_cdi_41.pdf`, Release 4.1, p. 9; local metadata-only inventories | `DiscInventory` records the XA Bridge signature, PVD identifiers, native `CDI` application presence, `video-cd` classification, entry points, and PSD topology without retaining media. A synthetic Mode-2 Bridge image covers `INFO`/`ENTRIES`/`LOT`/`PSD`; a local 2.0 disc reports 156 valid lists (129 selection, 27 play). Media-panel work preserves the on-disc application instead of bypassing it. | Correlate native engine control failures with the selected PSD list and first transport/decoder error. |
| Photo CD | A compliant Photo CD is a CD-i Bridge disc with an on-disc CD-i application; Base images are 768×512 square pixels (3:2), while informative soft-display diagrams distinguish NTSC/PAL and 0/5/10% overscan | `faq/cdifaq5.html`, §§5.9-5.9.2; Photo CD v0.9, IV.2.1 and Appendix II; external package `sw_app/photocd_on_cdi_32.zip` | The Photo CD crate parses `PHOTO_CD/INFO.PCD`, images, and playlists and presents host controls while retaining the CD-i application path. `DiscInventory` now independently records the XA Bridge signature, PVD application identifier, root `CDI` tree, and `photo-cd` classification; a synthetic Mode-2 Bridge fixture and local compliant disc both pass. A clean 768×512 source and native 768×588 capture prove that stationary field-like detail and a magenta final column arise after source storage; horizontal bars can be compatible with application-selected 3:2-to-4:3 presentation, but the supplied asymmetric 76/34-row placement still requires live-register evidence. | Capture guest drawmaps, consecutive fields, composed raster, and live display registers before classifying native bars or field artifacts. |

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

### Early full-motion architecture

Sijstermans and van der Meer's 1991 encoder paper predates the final CD-i FMV
profile, but it independently fixes several architectural intentions. It
budgets roughly 1.2 Mbit/s for video and 0.2 Mbit/s for audio, describes
I-picture entry points plus forward- and bidirectionally-predicted pictures,
and uses quantizer feedback and buffering to hold the compressed stream near a
fixed disc rate. Critically, the encoder reconstructs its own reference
pictures through the decoder path so that motion prediction sees the same
losses as the eventual player and does not accumulate an encoder/decoder
mismatch. Printed pp. 81-89.

The decoder sidebar says the player extracts timing information for audio and
real-time synchronization, reconstructs and reorders pictures, converts
24/25/30-picture material for 50/60 Hz display, and uses the base video system
for screen timing and pixel-by-pixel selection between FMV and base-case RGB.
Printed p. 90. These points corroborate the eventual CDIC/VMPEG/MCD251 path.
The paper's 360-sample early video example does not override the later
352-sample Green Book/TN 097 limits.

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

### Physical disc errors and authored recovery

TN 054 and TN 058 keep three layers separate. Every sector receives CIRC at
the physical-disc layer. Non-real-time Form 1 sectors can receive additional
CD-RTOS ECC, historically taking up to 300 ms in software, whereas a real-time
Form 1 sector must be corrected transparently or left uncorrected so real-time
delivery is not deliberately stalled. Form 2 can carry a four-byte EDC but has
no corresponding ECC payload. The local TN 054 scan begins at printed page 2,
so its missing first page must be recovered from another edition before claims
about its introduction are catalogued.

TN 058 clarifies that a non-real-time Form 1 sector may legally appear inside a
real-time file and be delivered asynchronously. If software correction is
needed, however, following audio or other time-critical sectors may cease to be
real-time. It also makes `PL_ERR` resolution player-dependent: byte-resolution
drivers require roughly one error bit per data byte (294 bytes for Form 2 and
256 for Form 1), while word-resolution drivers require half as much. The
application learns the resolution from `Err_Res` after an error rather than
from a CSD capability query.

These notes describe damaged physical media and authored recovery, not normal
clean-image transport. An image-backed emulator needs explicit error injection
and `PL_ERR` evidence before a native dirty-disc dialog can be attributed to
ECC, EDC, a bad image, or a later decoder failure.

### Disc seeking, audio delivery, and memory

TN 090 distinguishes logical and physical positioning. `I$Seek` changes the
file-position pointer; the next read performs any required physical seek.
`SS_Seek` moves the head immediately, so following it with an ordinary read
can cause a second move. Its intended benefit is overlapping a long seek with
other presentation, not making every read synchronous. A future head model
therefore needs distance-sensitive tests rather than a blanket delay.

TN 092 separates direct real-time audio from sound maps. Disc-fed audio runs
autonomously without consuming the CPU bus. For ADPCM, only selected sectors
advance the file-position pointer, although triggers may still fire for a
non-selected channel. Sound maps are memory/CPU fed and take audible priority
over direct play; `SM_Done` means transfer to the player's local buffer, not
that the sound has finished at the output. The undocumented local queue depth
is player-dependent, which makes a single hard-coded completion delay unsafe.

TN 087 records two 512 KiB colored base-memory banks and guarantees at least
480 KiB contiguous in each at application entry after OS structures. The DVC
adds its 1 MiB system RAM and 0.5 MiB priority-zero decoder RAM dynamically to
the live memory list, rather than through the static init module. These are
guest-visible allocation properties still requiring a boot trace, even though
the physical address ranges already exist.

TN 068 separates the NVRAM device from the native storage-manager policy. It
describes the base-case capacity as approximately 8 KiB and recommends an
adviser that first offers old files from the current application, then unknown
formats, then old files from other applications, before manual deletion. The
manual view supports sorting and file protection. Its displayed capacities are
percentages: a new file is rounded up, an existing file is rounded down, and a
small amount may deliberately be left unusable to avoid two unequal files both
appearing as the same percentage. This explains plausible shell presentation;
it is not hardware behavior to reproduce in the host frontend.

The 8 KiB statement agrees with the MK48T08 device held by the current core,
but the service manual also documents a larger address window and a
31-KiB-maximum configuration. That discrepancy is retained as a model/profile
question. It does not justify enlarging the current device, and the native
shell remains responsible for its filesystem, deletion policy, and UI.

TN 062 is a broad CD-RTOS 1.1 bug list dated February 1991. It is retained as
historical errata for named old player/software versions, not promoted to a
universal hardware contract.

TN 049 is earlier authoring-performance guidance rather than a device timing
specification. It corroborates a 15 MHz reference CPU, asynchronous real-time
play, inefficient partial-sector synchronous reads, and the usefulness of
`SS_Seek` for overlapping head movement. Its recommendations to cluster files,
preload, compress, and duplicate nearby data explain title layout choices; they
do not authorize caching or speculative reads inside the emulated player.

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

TN 105 adds a stricter authored-seamless-branch contract than the shorter
transition notes. A seamless branch keeps the decoders running because a
stop/restart reintroduces the first-SCR-to-first-PTS startup interval. The
introductory and body streams must meet at compatible VBV/STD buffer states,
enter a closed GOP, keep complete audio frames at branch boundaries, preserve
SCR/PTS/DTS continuity, and omit intermediate program-end codes. A sequence
end is used only when parameters change or at the final end. Run-time patching
must derive its delta from PTS, not SCR, and a PCL remains full until those
timestamps are patched. The existing transition tests prove decoder recovery;
they do not yet prove this authoring and run-time branch protocol.

### Historical authoring-player evidence

TN 096's Philips/Optimage “emulator” is not an independent software or CPU
implementation: it is a physical CD-i player whose CD drive is replaced by a
hard-disk source delivering data at exact CD speed. The CD-i 605 tools could
randomize video, audio, pointer, cursor, and timer registers, replay a prior
state, vary expansion memory from zero to 4 MiB, and force worst-case seek
delay. This is useful evidence for validation methodology and supported player
conditions, but not a second implementation whose behavior can settle an
ambiguous device rule.

The indexed-note pass also found two cautions. TN 046 internally contradicts
itself about whether plane A may program CLUT banks 2 and 3; the later MCD212
specification remains controlling until hardware evidence resolves the note.
TN 076 was OCRed from its scan, but this archive copy jumps from printed page 2
to printed page 4. Findings from the available pages are indexed; printed page
3 remains a replacement-source task rather than being reconstructed by guess.

### Composite output and international authoring

TN 039 explains a presentation effect rather than a digital decoder rule. At
CD-i's 7.5 MHz normal-resolution dot rate, narrow white or colored features can
become falsely colored or vary in brightness after composite NTSC encoding and
decoding. Single-pixel verticals and diagonals are the worst case; even
two-pixel detail may acquire a pale hue. The note explicitly applies this to all
CD-i image-coding methods. Large color areas are comparatively stable. E-Di's
raw framebuffer should therefore remain the clean digital raster. A future CRT
or composite simulation would be an optional presentation effect whose encoder
model must be stated, not a correction inside MCD212 decoding.

TN 048's three authoring levels keep another boundary clear. Every title must
program the display compatibility state from the type-3 CSD entry. A 240-line
525 title intended to remain acceptable on a 280-line 625 display can author 20
neutral scanlines above and below its image; the full compatible format is
384x280. The later Balboa mapping in TN 099 describes the same centering in
UCM coordinates. Those are title/manager policies. They corroborate the
current 240-in-280 aperture but do not authorize the emulator to crop a final
line or synthesize borders for a title that programmed something else.

TN 048 also recommends table-driven presentation timing for translated audio
and reserving real-time bandwidth when multilingual tracks are planned. Each
additional mono C-channel reservation costs about six percent of usable
bandwidth. This is authoring corroboration for independent channel selection,
not a reason for the emulator to change sector cadence.

### Balboa video-manager evidence

TN 099 is primarily an application-library note, but its examples expose useful
native contracts. Real-time-file input is asynchronous and arrives in whole
2324-byte Form 2 sectors. Balboa cycles through buffer lists independently for
each channel/type, while `pml_play()` separately selects RAM-delivery channels,
one direct-audio channel, and an EOR-delimited record count. This independently
corroborates the current `PCB_Chan`/`PCB_AChan`/`PCB_Rec` interpretation and
the need to keep buffer ownership visible until native cleanup.

The same note shows why partial-screen DYUV movies are authoring operations:
three rotating picture buffers receive sector-aligned frames, the unused pixels
to the left of a partial are initialized to zero, and mattes reveal the selected
buffer through the other plane. It also documents the eight ordered matte
registers. The later Green Book resolves one imprecise sentence in TN 099:
matte *flags* reset at each scanline, while matte-control registers persist from
line to line until the DCP reloads them. No end-of-field register clear should
be added from the application note alone.

The Green Book also gives path A priority when both control programs load the
same matte register simultaneously. MCD212 §5.4.4.12 confirms that channel 1
has first priority and channel 2 is ignored; the manual's control-loading
discussion defines simultaneous as occurring in the same line. The previous
line-level scheduler executed path A and then allowed path B to overwrite the
shared register. A failing paired-DCA fixture exposed that inversion. ICA and
DCA phases now retain the set of matte registers written by path A and ignore
only matching path-B loads; non-conflicting path-B loads still execute. The
fixture deliberately puts the conflicting loads in different command slots to
protect the line-wide scope. Companion tests cover the false line start,
persistent command template, one-/two-set order, and STOP.

TN 100 supplies the stronger scan-timing constraints. An interrupt command
notifies software when the display actually reads that LCT/FCT instruction;
editing an LCT does not make it visible immediately and may take up to one
field. Double-buffered LCT/FCT programs are the normal safe route. When an
application writes the active LCT, the 16-bit video-memory path can let the
display fetch old and new halves of one 32-bit command, producing a possible
one-field flash. The current line-level DCA execution is directionally correct,
but a within-line CPU-write/display-fetch fixture is required before using this
note to change scheduling or explain the Video CD lower-edge incident.

TN 100 also independently identifies audio/CD, video, and the 100 Hz system
timer as three drifting clocks. Its scrolling example derives motion from field
interrupts and explicitly adjusts 50 Hz versus 60 Hz increments. This supports
the existing separate-clock research boundary; it does not make one clock the
universal emulator time base.

### Cross-player compatibility boundaries

TN 085.1 treats the Green Book as a minimum player contract rather than a
promise that every development-system convenience exists. It confirms two
480 KiB contiguous base-memory minima, a fixed system tick anywhere from 99.8
to 100.2 Hz, three display types (384x240 NTSC TV, 360x240 NTSC monitor, and
384x280 PAL), four pointing-device classes, optional SCF, and manufacturer-
selected device names discovered through the CSD. Even `/nil` is not base case;
only `/nvr` has a mandated name. These constraints belong in player models and
guest discovery tests, not global Philips-name aliases.

TN 076 adds a concrete 605 example. Its CSD contains player-control and
extension-device entries absent from the 18x CSD. With one external pointing
device, that device becomes `/ptr`; a rear-port device also leaves the assumed
remote as `/pt2`; with no external device, the remote supplies the pointer CSD
entry. The 605 also uses different NTSC and PAL crystals, unlike the 18x's
single-crystal arrangement, and offers a 0-4 MiB extension-memory test setting.

Two compatibility warnings are especially useful diagnostically. First, an
illegal DCP has deliberately unspecified output: DYUV combined with four-bit
resolution can make both planes gray on a 220 even when the bad plane is
masked. Second, every raster line needs a valid linked LCT; losing the last
link makes the display processor fetch undefined memory. Neither case should
be cosmetically normalized. Record the guest program and scan phase before
deciding that a strange raster is an emulator defect.

Finally, the Green Book permits a `PCB_Rec` clear to be recognized on the next
selected sector, but TN 085.1 warns that some players wait specifically for a
selected sector delivered to RAM. Direct-to-audio delivery can therefore leave
the play running on those players. The note recommends `ss_abort` for reliable
termination and documents RAM-routing the audio channel as the title-side
workaround. This is a distinct selected-sector contract worth testing
alongside current CDIC stop behavior. The worked real-time-file example in the
Philips *master Disc Building Utility*, printed pp. 3-64--3-65, confirms that
`PCB_Rec` is an
EOR-delimited record count: setting it to one stops after one record, while two
permits two record boundaries without an intervening `ss_play()`. It also
separates `PCB_Chan`, which selects channels for processing, from `PCB_AChan`,
which routes selected audio directly to the audio processor. The diagnostic
tracker now records those fields and their transitions without taking over the
native CDFM driver's semantics.

Philips FPD805 supplies a native comparison without adapting its source. The
on-disc `bmp_nat` program selects channels 0, 15, and 16, routes channel 15
directly to audio, and requests three EOR-delimited records. Its uninterrupted
path returns after normal record exhaustion; its two-button handler calls
`ss_abort`. The deterministic local gate observes the abort path clearing the
direct-audio route and issuing CDIC Update about 15.8 million cycles earlier,
with 1,792,224 rather than 1,834,560 audio frames, before both paths return to
the same player state and framebuffer. Source review supplies a necessary
correction: `abort_bumper()` clears `PCB_AChan` during fade-down and
`play_bumper()` calls `ss_abort` only afterward, so the early route clear is an
application action rather than a property of `ss_abort` itself.

The exact-FPD805 `PCB_Rec` matrix deliberately patches the live final-bumper
PCB and records every host action in diagnostics. It preflights both the disc
fingerprint and system-ROM hash, then verifies the expected before/after hashes
for each live PCB field. After isolating channel 15, it changes `PCB_Rec` from
one to zero while leaving direct audio enabled, then repeats TN 085.1's portable
sequence by clearing `PCB_AChan` immediately before `PCB_Rec`, once with
`Audio CIL[15] == NULL` and once with a valid one-sector PCL. The reused PCL is
initialized and published only after its former video channel is inactive. The
current 220 ROM recognizes all three at the same next selected audio sector.
The two workaround cases have identical `PCB_Rec`, selected-sector,
route-clear, audio-frame, framebuffer, and final-PC timelines. That is a
permitted fast player result, not proof that every CD-i driver behaves this
way; TN 085.1 explicitly documents slower players for which the workaround is
necessary.

### RGB555 and DYUV storage/scanline boundaries

TN 022 resolves an important format-boundary ambiguity. RGB555 pixels in a
CD-i IFF file are planar: all upper bytes for plane A precede all lower bytes
for plane B. In contrast, the UCM `dm_write()` API accepts interleaved 16-bit
pixels and splits them between drawmap banks. Rectangular IFF scanlines include
longword padding, and real-time storage aligns both byte banks independently to
sector boundaries. These are authoring/storage rules. The MCD212 still sees
two drawmap planes, and the current renderer already combines plane B as the
low byte with plane A as the high byte.

TN 034 and TN 086 independently pin DYUV decoding to the same fixed 16-entry
delta table and modulo-256 arithmetic. More importantly, programmed Y/U/V
start values are reapplied at the beginning of every scanline; decoder state
must never leak from one line into the next. Horizontal panning is therefore
an authored transformation: it advances in two-pixel units and recomputes a
start value for every affected line. Likewise, the eight-pixel transition zone
used by older DYUV blitting tools, and the later fitted-edge search, prepare
media to meet the decoded background at both edges. They are not display-chip
effects for the emulator to synthesize.

Code review found the exact table, wrapping addition, per-line start reset, and
RGB555 byte-plane order already present. Focused device regressions now force
all three contracts and pass without an emulation behavior change.

### UVLO is a native software pipeline

TN 053 describes UVLO as a non-Green-Book, title-supplied compression method
for base-case motion video. It reduces chroma resolution beyond ordinary DYUV,
but the player does not expose a UVLO image-coding mode. Guest 680x0 code
decodes each UVLO frame into a normal DYUV drawmap. Specialized FMV hardware
uses a different coding scheme, so UVLO media must not enter E-Di's VMPEG path
or become a new host-side MCD212 decoder.

The sample application is useful cross-subsystem evidence. It receives one
frame per fixed number of Form 2 sectors through four circular PCLs, resets
`PCL_Cnt`, `PCL_Ctrl`, and `PCL_BufSz` after each buffer-full signal, decodes
into two alternating DYUV drawmaps, and changes the active plane-A display
address through the LCT. Plane B supplies the background, and three matte
instructions reveal the UVLO rectangle. When producer and consumer meet, the
application counts a missed frame and advances its decode queue. Its 384-pixel
display helper reads the CSD and selects
Compatibility Mode 0 for TV output or Mode 1 for monitor output.

This independently corroborates existing PCL ownership, drawmap, matte, and
TV/monitor boundaries but does not by itself require a behavior change. The
document's throughput tables are explicitly planning estimates: at higher
frame rates the 75-sector stream limits screen area, while UVLO decoding can
consume nearly all available CPU throughput. A native or redistributable UVLO
fixture would therefore be a useful combined CPU/CDIC/MCD212 timing gate; the
theoretical screen-utilization percentages are not emulator timing constants.

### Display and region observations

TN 93 calculates theoretical pixel-height/width ratios of about 1.19 for 525
and 1.05 for 625, then reports measured Philips-player values of 1.225 and
1.025. TN 104 later calculates 1.230 and 1.017 and reports measured values of
1.235 ± 0.003 and 1.019 ± 0.003. This approximately one-percent disagreement
is preserved as a calibration dispute. TN 104 separately gives White Book
source-pixel ratios of 1.1069 for NTSC and 0.9157 for PAL and says the decoder
ignores the MPEG aspect-ratio field; those asset ratios must not be substituted
for the player's output correction. TN 93 explicitly warns that a bitmap can
look vertically stretched on a 525 player or compacted on a 625 player.
Dedicated regional
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

TN 042 confirms that true odd/even interlaced drawmaps are part of the base
case. High-contrast horizontal detail can therefore produce real interline
flicker; line repetition and an authoring-side vertical filter are different
operations. This is evidence for an interlaced test fixture, not permission to
silently filter the emulated MCD212 output. TN 089 likewise says scan-line
signals and events have variable latency; a system-state `F$Event`
interception is a guest technique for clean transitions, not device behavior
to synthesize in the emulator.

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

### Native control-bar lower edge (disputed)

Release 4.1 specifies that its play control bar initially appears at the bottom
of the screen, but it also says that the application is configurable. It does
not specify an invariant number of covered display rows or a universal
composition rule for every release/configuration. Consequently, Addams Family
Values UK having a clean, flush lower edge is useful counterevidence to a
unconditional emulator fault, but it does **not** establish a different Video
CD standard. The next evidence is the title's engine/application fingerprint
and active display state, not a regional assumption.

The MCD251 technical summary describes decoded MPEG output (including a
programmable border) as pixel-by-pixel multiplexed with the base video decoder;
its display-control buffer includes active, display, and window coordinates.
`docs/mcd251ts.pdf`, Rev. 0, printed pp. 2-9--2-10. A colored row under a
native bar can therefore arise at the MCD251/MCD212 composition boundary even
when the decoded picture contains the row; that is consistent with the paired
Addams captures, where noisy plane-B data exists both before and after the
CONTENTS round trip but is only exposed in the failing composed raster. It is
not proof that the row is an emulator defect or intentional authoring.

Two primary notes make one-field or state-transition display artifacts plausible
without making them expected. TN 063 says that two plane-control structures are
read in synchrony while CPU updates are sequential; unsynchronised updates can
briefly combine one new plane with one old plane. TN 069 further records that a
910 executes the first linked LCT immediately after the FCT, unlike the Green
Book model. `notes/technote063.pdf`, printed pp. 1-4; `notes/technote069.pdf`,
printed p. 8, §3.3. These historical player notes are not authority to emulate a
910-specific quirk on a CD-i 220, but they rule out a cosmetic crop or a
field-wide interpretation of DCA STOP as evidence-led fixes.

For the current Addams USA incident, preserve plane A, plane B, composed-raster,
MCD251 window/register state, and final aperture for the same video time before
and after a transition that clears the line. A hardware capture using the same
engine revision is the deciding comparison. Until then, status remains
`disputed`; no lower-edge masking or DCA lifecycle change is justified.

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

### Photo CD source and soft-display geometry

Photo CD v0.9 defines the Base image as 768×512 square pixels, so the stored
image has a 3:2 aspect ratio. Appendix II is explicitly informative rather
than normative. Its diagrams show typical mappings of that image into 4:3
NTSC and PAL soft displays at 0%, 5%, and 10% overscan. At 0% overscan, the
landscape Base example extends horizontally beyond the NTSC screen but sits
inside the PAL screen vertically. Increasing overscan changes those visible
borders. A native Photo CD application may therefore show horizontal bars
without proving an MCD212 aperture defect; the live application mode and
display registers decide which mapping applies. In the supplied native raster,
however, near-black padding measures 76 rows above and 34 below. The 3:2 source
ratio explains possible total padding, not that asymmetric placement.

A later four-way comparison isolates the host viewer from that native-display
question. `IMG0006.PCD` decodes to the specified 768x512 Base canvas. The host
**View Raw Images** path preserves its normalized horizontal content position
and width to within 0.1%, so its symmetric horizontal bars are the expected
result of fitting the complete 3:2 source inside the available display area.
The native CD-i path and an analog hardware capture both instead match the
center crop required to fill 4:3: retain 682 2/3 of 768 source pixels (remove
about 42 2/3 pixels, or 5.56%, from each side) and enlarge the retained image by
9/8. A warm-content feature spanning 42.58% of the source spans 42.64% in the
raw viewer, 48.44% in native E-Di output, and 47.62% in the hardware capture;
the predicted cropped width is 47.90%. This requires no vertical stretch.
Consequently both presentations are valid for their stated purposes: the raw
viewer exposes the complete square-pixel source, while the disc application
produces a television-oriented soft display. Do not use the raw viewer's bars
as evidence of an MCD212 defect or silently crop its source-pixel mode.

This does not explain the reported stationary comb-like detail or saturated
magenta final column. The supplied source is a clean 768×512 Base image, while
the native CD-i capture is a 768×588 presented raster containing the detail.
Its last two output columns are magenta-like over 382 rows, consistent with one
15 MHz source sample being doubled at the 30 MHz output boundary but not proof
of which stage supplied that sample.
The specification supplies no rule that adds such artifacts to a still.
Display provenance must locate their first appearance after storage: decoded
source, guest PCL/drawmap, odd/even MCD212 planes, composed raster, aperture,
or frontend copy.

The field rules narrow that investigation. MCD212 section 5.2.2 defines an
interlaced picture as odd display lines supplied by the odd field and even
display lines supplied by the even field. Table 5-8 selects channel 1 ICA byte
address `$400` for odd `PA=1` and `$404` for even `PA=0`. Green Book V.4.5.1
requires a field-control table before each field and warns that display
parameters are not guaranteed to survive from one field to the next. Green
Book V.5.14 permits the same data in both fields for normal/double-resolution
pictures and requires interlace for high-resolution FCT/LCT pictures.

Philips TN 042 adds an important presentation caveat: a still can legitimately
be built from distinct odd/even field data, but high-contrast horizontal detail
or simple line repetition produces objectionable interline structure on a CRT.
It recommends vertical filtering/interpolation (with a typical adjacent-line
weight around 0.2–0.33) at authoring time. A lossless progressive weave shows
both fields simultaneously, so such authored field structure can appear as a
stationary one-line artifact in a PNG even though a CRT would present and
optically blend the fields at different times. This is a hypothesis to test,
not permission to add a global deinterlacer: high-resolution field detail is
valid CD-i output and must not be discarded without provenance evidence.

The first core audit found no error in ICA selection: `process_ica` selects
`$400`/`$404` according to Table 5-8, parity toggles once at field completion,
and the complementary field is retained. It did not, however, independently
prove which progressive host row should retain each PA phase. Two four-field
provenance captures cover a non-PCD startup graphic and an actual photograph
displayed by the native application. Both affected scenes program interlaced
768×480 output and switch both MCD212 planes to DYUV. The artifacts occur
throughout the picture, not only at its horizontal boundaries; the boundaries
merely make the one-line displacement especially visible.

Reconstructing the visible VSR sequence from both fields exposed the missing
distinction. With the old host-row phase, the composed row order repeatedly
stepped backward by one `$480`-byte source line at PA boundaries in both
planes. Retaining PA=0 in the first row of each progressive pair and PA=1 in
the second makes the source order monotonic without changing the Table 5-8 ICA
entry points. An offline composite made with only that row-phase swap was
identified by the user as correct and exactly matched the returned reference
screenshot by SHA-256. A device-level regression now covers the phase while
preserving noninterlaced row duplication. Live native-application verification
confirmed that the startup, stable menu, and displayed photograph are clean.
The remaining field structure is confined to a moving Photo CD transition
line/area. Other tested CD-i transitions, including Philips FMVDemo's VMPEG
external-video path, do not show it, so that partial-update symptom is tracked
separately from the resolved stable-picture weave defect.

The right-edge question now has a device-level answer. MCD212 section 7.1 says
that its horizontally subsampled DYUV output interpolates missing U/V values,
but the last missing U or V component on a line is obtained by repeating the
last component. E-Di instead peeked at the next two bytes when expanding the
line's final pixel pair, allowing next-line data to color the last two output
pixels. A synthetic regression test varied only those following bytes and
failed before the correction; it now proves that a completed line is
independent of subsequent data. This correction targets the magenta right edge
only and still requires native Photo CD confirmation.

The captures also establish that PCD source decoding alone is not the cause of
the separate stationary field artifacts, because
the non-PCD startup graphic uses the same affected dual-DYUV path. The visually
clean intervening CD-i menu supplies the needed control. Its four captured
fields are stable and byte-identical, but it programs non-interlaced 768×480
output with a CLUT4 icon/control plane over a DYUV background plane. That is
materially different from the affected interlaced dual-DYUV startup and
photograph regions. The symptom is therefore correlated with the native
application's interlaced dual-DYUV path, not Photo CD source decoding, the
hardware aperture, or the frontend generally. The corrected retained-row phase
addresses that generalized MCD212 path; no global deinterlacer or host crop is
used.

Similar-sized bars in ordinary European CD-i titles are a comparison lead,
not evidence of one root cause. PAL raster/compatibility modes, authored
240-line assets, and Photo CD's 3:2 source fitting are separate mechanisms and
must be distinguished by live MCD212 state.

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
7. **Video CD filesystem/PSD (complete):** the synthetic CD-XA image covers
   Philips engine paths, entry points, LOT offsets, and selection/play/end
   topology. Use that metadata to correlate native control failures.
8. **525 monitor boundary:** table-driven geometry tests only when a monitor
   player model is introduced.
9. **Sound-map completion:** separate transfer-done from audible completion
   and test stop/replacement tails.
10. **Keyboard peripheral:** K-mode ID, press/release, modifier, and idle-silence
   packet tests before exposing host passthrough.
11. **Photo CD Bridge inventory (complete):** the documented filesystem,
    Bridge markers, PVD application identifier, and native `CDI` tree are
    classified without extracting image payloads; host controls continue to
    preserve native application behavior.
12. **Native Photo CD display provenance:** compare a clean 768×512 source
    against guest drawmaps, four consecutive fields, composed output, and the
    hardware aperture. Record live standard, `CF`/`ST`/`FD`/`SM`, origins, and
    magnification so legitimate 3:2-to-4:3 borders remain separate from field
    or last-column corruption.
13. **Physical sector-error contract:** inject correctable and uncorrectable
    Form 1/Form 2 errors, populate byte- and word-resolution `PL_ERR`, and
    prove software correction can delay following real-time delivery.
14. **Player CSD matrix:** parse live zero/one/two-device boot configurations
    and compare `/ptr`, `/pt2`, `/pck`, extension entries, and manufacturer-
    selected names without hard-coded Philips aliases.
15. **Selected-sector play termination (current 220 gate complete):** PCB
    discovery and record/routing transitions are covered. Philips FPD805 now
    distinguishes normal exhaustion from its abort path and tests live
    `PCB_Rec = 0` during direct audio plus TN 085.1's zero/nonzero audio-CIL
    workaround. An alternate player/driver fixture is required before modeling
    the slower compatibility phenotype documented by TN 085.1.
16. **Undefined DCP diagnostics:** identify missing final LCT links and illegal
    transient mode combinations while leaving their raster unspecified.
17. **RGB555/DYUV raster contracts (complete):** asymmetric plane bytes prove
    plane-A/high plus plane-B/low RGB555 assembly; separate fixtures force
    modulo-256 DYUV wrap and fresh programmed Y/U/V starts on consecutive
    scanlines. IFF/UCM conversion remains outside the device.
18. **NVRAM capacity/profile:** only if a title or shell diverges, probe the
    native address window and near-capacity filesystem behavior to distinguish
    8 KiB MK48T08 storage from the service manual's larger configuration and
    to verify NVRUI rounding/protection/deletion policy.
19. **DCP live-update timeline:** place an interrupt command and a deliberately
    split 32-bit LCT write around one display fetch. Record the active DCA slot,
    IT edge, both 16-bit halves, and affected raster line. The separate matte
    fixture is complete: flags restart false, commands persist and execute in
    order, STOP terminates a set, and a simultaneous shared-register load gives
    path A priority. Do not use the remaining synthetic race as a title
    workaround.
20. **Native UVLO pipeline:** use a redistributable fixture, or an optional
    local Engineering 6.0 case if present, to verify circular PCL ownership,
    deliberate frame dropping under decoder pressure, alternating DYUV
    drawmaps, matte bounds, and CSD-selected 384-pixel TV compatibility mode.
    Keep UVLO in guest software rather than adding a VMPEG or host codec path.

The full 185-PDF archive now has a validated searchable-text manifest. Research
remains open while lower-priority player/service-manual claims are reviewed.
New findings should extend the compliance matrix with a source and a falsifying
test; they should not directly become compatibility constants.
