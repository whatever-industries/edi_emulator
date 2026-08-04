# CD-i Specification and Evidence Index

This index identifies the authority to consult before a device change.
Generated OCR text is local and ignored; tracked citations must include the
document edition, section/page, and source hash from the OCR manifest.

Use `docs/specification-research.md` for the current human-readable research
ledger, evidence interpretations, and prioritized falsifying tests.
`docs/icdia-archive-assessment.md` remains the historical archive survey; its
implementation-status statements are not authoritative after later code
changes.

| Subsystem | Primary documents | Implementation entry points | Current evidence/tests |
|---|---|---|---|
| Disc/filesystem/RTF | Green Book III.4, III.5, V.6, VII CDFM; `docs/cddiscgeneralrcdiexhIII.pdf`; TN 054/058 error recovery; TN 090 seek semantics | `cdi-disc::inventory`, `cdi-core::cdic` | filesystem/sector inventory unit tests; CDIC channel/form tests |
| CPU/peripherals | SCC68070 User Manual and Philips section-6.2 timing notes | `cdi-scc68070`, `cdi-core::machine` | 118,187 Harte vectors; DMA/IRQ tests |
| Base video | `docs/mcd212rev0.pdf`, Green Book V.2/V.4/V.5/V.6, TN 022/034/039/042/048/053/085.1/086/089/099/100/104 display notes | `cdi-core::mcd212` | table-driven geometry/DCA/interlace, RGB555/DYUV line contracts, and matte ordering/path-priority tests; live-LCT race and native UVLO software-pipeline fixtures pending |
| CDIC/audio | Green Book III/VII, CDIC technical data, CDFM documentation, TN 092 audio delivery | `cdi-core::cdic` | XA/CDDA/sound-map/filter tests |
| SLAVE/input | HC05 firmware, `docs/pointing_devices.pdf`, `docs/mc68hc05c8rg.pdf`, TN 076/085.1 CSD and player-model notes | `cdi-core::slave` | `docs/slave-protocol.md` and protocol tests |
| VMPEG/DVC | `svcmanuals/22er9141.pdf`, `docs/mcd251ts.pdf`, `docs/fmv_extension.pdf`, `docs/cdi_fmv_rec.pdf`, Green/White Book, TN 097/103/105 | `cdi-core::dvc`, `mpeg1_video` | `docs/mpeg-dvc-plan.md`; demux/register/timing tests |
| VCD | White Book plus `authoring/vcd_introduction.pdf`, `authoring/vcd_synopsis.pdf`, `docs_sw/vcd_on_cdi_*.pdf` | disc inventory, CDIC, DVC | planned VCD pilot incident |
| CD-RTOS/CDFM | `microware/cdisys.pdf`, `authoring/cdi_standards.pdf`, `authoring/master.pdf`, Green Book VII, TN 049/053/054/058/062/085.1/087/090/099 | guest PCB/CIL/PCL structure tracing | OS-9 module inventory; runtime PCB/PCL state and ownership diagnostics |
| UCM/drawmaps | Green Book V.6, TN 022 RGB555 storage, TN 034 DYUV panning, TN 086 DYUV encoding/blitting | MCD212 and planned drawmap provenance | raster/aperture tests; IFF/API layout and buffer-to-drawmap tracing gaps |
| NVRAM/timekeeper | MK48T08 data/device evidence, service-manual memory maps, TN 068 NVRUI policy | `cdi-core::machine`, frontend storage | 8 KiB device mapping/persistence tests; native capacity/profile fixture pending |
| Keyboard | `docs/keyboards_1996.pdf`, `docs/keyboard_drivers.pdf` | planned serial input-device boundary | documented K/T packet fixtures pending |
| Photo CD | `faq/cdifaq5.html`, `sw_app/photocd_on_cdi_32.zip` | `cdi-photocd`, disc inventory, frontend | host viewer implemented; Bridge/native-application classification pending |

The OCR script defaults to this checkout's current local ICDIA research mirror.
Set `$CDI_REFERENCE_ROOT` when the mirror is elsewhere:

```sh
export CDI_REFERENCE_ROOT=/path/to/icdia-site-documents
```

Useful exact files include:

- `docs/mcd212rev0.pdf`
- `docs/mcd251ts.pdf`
- `docs/cddiscgeneralrcdiexhIII.pdf`
- `docs/fmv_extension.pdf`
- `docs/cdi_fmv_rec.pdf`
- `docs/pointing_devices.pdf`
- `docs/keyboards_1996.pdf`
- `svcmanuals/22er9141.pdf`
- `svcmanuals/cdi205.pdf`
- `microware/cdisys.pdf`
- `authoring/cdi_standards.pdf`
- `authoring/master.pdf`
- `authoring/vcd_introduction.pdf`
- `authoring/vcd_synopsis.pdf`
- `docs_sw/vcd_on_cdi_41.pdf`
- `notes/techindex.pdf` and the numbered `notes/technote*.pdf` collection.

## Compliance status vocabulary

- `implemented-tested`: device behavior and a focused test are linked.
- `implemented-unverified`: code exists but lacks adequate source/hardware
  verification.
- `partial`: only a documented subset exists.
- `gap`: not implemented.
- `disputed`: sources or observations conflict; incident required.

The live compliance matrix is `data/compatibility/compliance-matrix.json`.
Update it with citations and test symbols when implementation status changes.
Narrative findings whose tests have not yet been implemented belong in
`docs/specification-research.md`; add them to the JSON matrix as `gap`,
`partial`, or `disputed` so the debugging tools can retrieve them.

## Reproducible local text extraction

Run with the current local default:

```sh
scripts/ocr-cdi-specs.sh
```

Or pass a relocated mirror explicitly:

```sh
scripts/ocr-cdi-specs.sh \
  "$CDI_REFERENCE_ROOT" \
  references/spec-text
```

The ignored output includes a SHA-256 manifest. The script uses embedded text
when credible and 240 dpi Tesseract OCR otherwise. Do not commit the source
documents or generated full-text corpus.

The 2026-08-03 complete run covers all 185 PDFs in the current mirror with 185
unique source paths and hashes and no missing text sidecars: 141 embedded-text
extractions, 23 Tesseract OCR sidecars, and 21 validated reused sidecars.
