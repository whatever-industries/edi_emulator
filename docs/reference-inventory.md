# Reference inventory checkpoint

Status: 2026-08-08. This is an extraction inventory, not a claim that every
document has been read or that every extracted statement is authoritative.
Generated text remains ignored under `references/` or `tmp/`.

## Collections

| Collection | Current extraction state | Value | Known gap |
|---|---|---|---|
| Local ICDIA mirror | 405 non-metadata files (411 including six `.DS_Store` files): 185 PDFs, 123 HTML files and 15 ZIP archives. All 185 PDFs have text sidecars listed in `references/spec-text/manifest.tsv`: 141 by `pdftotext`, 23 by 240-dpi Tesseract, and 21 reused local extractions. | Main Philips specification, technical-note, service-manual, authoring and Microware corpus. `docs/specification-index.md` routes subsystem work; `docs/specification-research.md` records assessed findings. | PDF extraction completeness is not reading completeness. HTML and archive contents are catalogued only selectively. The mirrored Green Book HTML index omits linked extensionless chapters; a recovered older archive is local/ignored. |
| Supplemental `disc specs/Philips CD-i` PDFs | Five PDFs: `BRIDGE10.pdf`, `CD-I full-motion video encoding on a parallel computer.pdf`, `DSA2IH00106115.pdf`, `Motorolla Multimeda Device Data.pdf`, and `cdi_may94_r2.pdf`. Four have embedded text; `DSA2IH00106115.pdf` has a local OCR extraction. | Later/larger editions and documents supplied outside the site mirror, including the 408-page Motorola device collection and 1,000-page Green Book scan. | They are not all represented in the ICDIA OCR manifest. Page-level assessment is selective, and the MCD251 material remains only a technical summary rather than the requested full data sheet. |
| Photo CD specifications | `disc specs/Photo CD/Photo CD.pdf` has embedded text and has been selectively assessed. | Photo CD Bridge structure and 768x512 Base image/presentation evidence. | Native CD-i Photo CD application behavior still needs live-register/drawmap correlation for field-transition questions. |
| Philips developer discs | FMVDemo, FPD804 and FPD805 sources are local diagnostic inputs; useful files have been extracted selectively and findings are recorded in the research ledger. | Native VMPEG driver usage, PCL flow, positioning calls, and application examples. | No complete source-tree index or provenance map exists. Disc images and extracted commercial/unrelated payloads stay untracked. |
| Firmware and disassembly | Bundled supported system/DVC firmware is scanned by `cdi-os9`; tracked HC05 disassemblies and protocol notes cover selected SLAVE/SERVO paths. | Exact ROM modules, CSD records, reset flow, input and drive-protocol evidence. | Physical SERVO timing/state-machine coverage and live OS-9 memory/process structures remain incomplete. |
| External emulator/reference projects | Pinned reviews and review dates live in `data/external-project-reviews.json`; update checks use `scripts/check-external-project-updates.py`. | Independent comparison and falsifying tests, subject to the project-specific legal boundaries in `AGENTS.md`. | A newer upstream HEAD is not a substantive review. Re-review is needed only when relevant upstream changes exist. |

## Reproducibility

- `scripts/ocr-cdi-specs.sh` reproduces the 185-PDF ICDIA text corpus and
  manifest; it does not OCR every other local collection.
- `docs/specification-index.md` is the authority router.
- `docs/specification-research.md` is the current human-readable evidence
  ledger.
- `data/compatibility/compliance-matrix.json` links findings to implementation
  and tests.
- `docs/icdia-archive-assessment.md` is a historical survey and may describe
  implementation state that has since changed.

## Exact coverage gaps for the next research checkpoint

1. Pilot a page/section index over one bounded subsystem, then measure whether
   it improves source retrieval before expanding beyond the existing PDF OCR.
2. Obtain or independently measure the missing MCD251 coordinate/output rules;
   do not infer them from the technical-summary register names.
3. Build a selective index of the developer-disc source files used as evidence,
   including hashes and the findings they support.
4. Map the relevant OS-9 System Globals, process/scheduler and CDFM/UCM
   structures by ROM revision before exposing additional runtime diagnostics.
5. Reconcile the later Green Book PDF, older recovered Green Book text and
   technical notes where editions differ; record conflicts instead of silently
   choosing whichever wording matches current behavior.
