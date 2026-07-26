# Compatibility evidence

This directory contains sanitized technical records only. Local incidents,
paths, logs, patches, screenshots, audio, ROMs, discs, and extracted title
media belong under the ignored `tests-data/local/diagnostics/`.

Tracked incident records use schema version 1 from `cdi-cli diagnose`.
Experiment outcomes distinguish a falsified hypothesis from a failed
implementation and carry applicability/invalidation conditions. A prior
result informs later work but never automatically blocks repetition.

Every new incident records the exact commit at report time. A diagnostic run
records the last reproduced commit only when explicitly confirmed with
`--symptom-reproduced`; successful process exit is not treated as reproduction.
Accepted manual checks record the last verified commit through
`diagnose verify --accepted`. `evidence_status: needs-revalidation` means the
observation predates a material emulator change and may still be correct, but
must not be presented as current behavior until it is repeated.

`TODO.md` is the short roadmap, not the experiment ledger. Its compatibility
items link here; detailed local logs, captures, and temporary patches remain
under ignored `tests-data/local/diagnostics/`.
