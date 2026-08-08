# Mono-I hardware boundaries

This is the short ownership map for the currently emulated CD-i 220/Mono-I
player and optional 22ER9141 VMPEG cartridge. It defines where an observed
effect belongs before implementation work begins. Detailed evidence remains
in the subsystem ledgers linked below.

## Data flow

```text
guest software / CD-RTOS
  |-- SPI commands and status --> SLAVE --> SERVO --> tray, spindle,
  |                                             seek and media classification
  |
  `-- CDFM requests and PCLs --> CDIC/DSP <-- internal drive X-Bus sectors
                                  |
                                  |-- DMA0 --> main RAM --> CDFM/PCL/UCM
                                  |                         |
                                  |                         `--> drawmaps
                                  |                               |
                                  |                               `--> MCD212
                                  |
                                  `-- DMA0 --> main RAM --> DMA2 --> VMPEG FIFO
                                                                  |
                                              MPEG video/audio decoders
                                                                  |
                                      MCD251 source window and position
                                                                  |
                           external 15/13.5 MHz VMPEG clock conversion
                                                                  |
                                                       MCD212 external plane
                                                                  |
                                              final composition and aperture
                                                                  |
                                                 frontend presentation
```

Audio follows the same transport distinction. CDIC receives CD-DA/XA data and
the VMPEG cartridge decodes MPEG Layer-II audio; CPU-visible SLAVE commands
control mute and attenuation at the player output boundary. The exact shared
analog path for base-case and DVC audio remains unresolved and must not be
inferred from the digital mixer alone.

## Ownership table

| Boundary | Owns | Does not own | Primary evidence |
|---|---|---|---|
| SLAVE/SERVO | CPU-visible drive commands and status; tray/spindle/seek mechanism; media classification; host-reset request; input packets and player output controls | Sector payload delivery, CDFM buffering, MPEG decode, or video composition | CD-i 220 service manual; CD-i 910 training pp. 13-14; SLAVE and SERVO firmware; `docs/slave-protocol.md` |
| Drive X-Bus, DSP and CDIC | The internal sector path into the player; sector selection and CDIC buffers; CD-DA/XA transport; DMA0 transfer and related interrupts | Tray policy, guest drawmaps, MPEG picture placement, or host audio delivery | Green Book III/VII; CDIC data; CD-i 910 training; TN 092/094; `docs/icdia-archive-assessment.md` |
| CDFM, PCL and UCM | Guest-visible files, channel selection, PCBs/CILs/PCLs, buffer ownership, real-time records, drawmap creation and transfer | Physical drive timing, decoder registers, final scanout, or frontend cropping | Green Book V.6/VII; CD-RTOS system calls; TN 053/090/099; `docs/debugging-workflow.md` |
| VMPEG and MCD251 | MPEG system/audio/video decode, decoder FIFO/state, decoded-picture timing, source window, display origin and active-area registers | Disc reads, the White Book output-clock conversion, base-plane composition, or host scaling | 22ER9141 service manual; MCD251 Technical Summary; Green Book IX; TN 097/098/101/103/105; `docs/mpeg-dvc-plan.md` |
| External VMPEG clock circuitry | Selection of the normal 15 MHz or White Book 13.5 MHz output path and its `15/13.5` (`10/9`) horizontal expansion before MCD212 | MCD251 source-window selection, a hidden phase control, vertical geometry, or frontend zoom | VMPEG service material; White Book; Interactive Engineer 96/05; observed native type-4 control described in `docs/mpeg-dvc-plan.md` |
| MCD212 | Base planes, cursor, mattes/transparency, external-video selection and final digital raster/aperture | MPEG decode, disc classification, title-specific cropping, CRT overscan, or window aspect policy | MCD212 data sheet; Green Book V; `docs/display-geometry.md` |
| Frontend | Presenting the core aperture, pixel-aspect correction, optional deinterlacing, host input mapping, screenshots and host audio delivery | Guest-visible device state, content-dependent crop, title timing, disc classification, or emulated composition | `docs/display-geometry.md`; frontend/core boundary tests |

## Reset and insertion boundary

A title chosen from the host library may be attached as power-on media and
followed by a full machine reset. That is a frontend policy for replacing one
independent title with another, not an emulated tray transaction. When guest
software opens the tray, SLAVE must command the mechanism through SERVO; the
event must not itself power-cycle the emulated player. A host reset requested
by SLAVE preserves the SLAVE state that selected the next boot mode. See
`docs/slave-protocol.md` for the observed command exchange.

## Unresolved boundaries

These are research questions, not implementation guidance:

- Which physical SERVO delays and failure transitions apply to each supported
  drive revision? Current HLE delays are compatibility behavior, not measured
  hardware timing.
- What are the exact X-Bus arbitration and drive-to-DSP/CDIC buffering rules
  when CDFM reuses a PCL late?
- Which live OS-9 globals are stable enough to expose CDFM PCB/CIL/PCL and UCM
  drawmap provenance without depending on one ROM revision?
- Do base-case audio and VMPEG audio share the same final SLAVE-programmed
  analog attenuation/mute path, and what is its measured transfer function?
- What rounding and origin behavior does the external 13.5 MHz converter use
  for every MCD251 position/window combination? The `10/9` scale is supported;
  a second phase register is not.
- Which MCD212 field/half-line transitions explain the remaining transient
  interlace and border incidents? Resolve this in core timing, not by
  content-aware frontend presentation.

