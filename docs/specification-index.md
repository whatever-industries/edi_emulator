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
| Disc/filesystem/RTF | Green Book III.4, III.5, V.6, VII CDFM; `docs/cddiscgeneralrcdiexhIII.pdf` | `cdi-disc::inventory`, `cdi-core::cdic` | filesystem/sector inventory unit tests; CDIC channel/form tests |
| CPU/peripherals | SCC68070 User Manual and Philips section-6.2 timing notes | `cdi-scc68070`, `cdi-core::machine` | 118,187 Harte vectors; DMA/IRQ tests |
| Base video | `docs/mcd212rev0.pdf`, Green Book V.2/V.4/V.6, TN-093 picture-quality notes | `cdi-core::mcd212` | table-driven geometry/DCA/interlace tests |
| CDIC/audio | Green Book III/VII, CDIC technical data, CDFM documentation | `cdi-core::cdic` | XA/CDDA/sound-map/filter tests |
| SLAVE/input | HC05 firmware, `docs/pointing_devices.pdf`, `docs/mc68hc05c8rg.pdf` | `cdi-core::slave` | `docs/slave-protocol.md` and protocol tests |
| VMPEG/DVC | `svcmanuals/22er9141.pdf`, `docs/mcd251ts.pdf`, `docs/fmv_extension.pdf`, `docs/cdi_fmv_rec.pdf`, Green/White Book | `cdi-core::dvc`, `mpeg1_video` | `docs/mpeg-dvc-plan.md`; demux/register/timing tests |
| VCD | White Book plus `authoring/vcd_introduction.pdf`, `authoring/vcd_synopsis.pdf`, `docs_sw/vcd_on_cdi_*.pdf` | disc inventory, CDIC, DVC | planned VCD pilot incident |
| CD-RTOS/CDFM | `microware/cdisys.pdf`, `authoring/cdi_standards.pdf`, Green Book VII | guest structure tracing (planned) | OS-9 module inventory; runtime PCL tracing gap |
| UCM/drawmaps | Green Book V.6 and UCM manuals/technical notes | MCD212 and planned drawmap provenance | raster/aperture tests; buffer-to-drawmap tracing gap |
| Keyboard | `docs/keyboards_1996.pdf`, `docs/keyboard_drivers.pdf` | planned serial input-device boundary | documented K/T packet fixtures pending |
| Photo CD | `faq/cdifaq5.html`, `sw_app/photocd_on_cdi_32.zip` | `cdi-photocd`, disc inventory, frontend | host viewer implemented; Bridge/native-application classification pending |

The local ICDIA mirror normally resides at:

```text
/Volumes/Projects/Coding/disc specs/Philips CD-i - icdia-site-documents-2026-07-18
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

Run:

```sh
scripts/ocr-cdi-specs.sh \
  "/Volumes/Projects/Coding/disc specs/Philips CD-i - icdia-site-documents-2026-07-18" \
  references/spec-text
```

The ignored output includes a SHA-256 manifest. The script uses embedded text
when credible and 240 dpi Tesseract OCR otherwise. Do not commit the source
documents or generated full-text corpus.
