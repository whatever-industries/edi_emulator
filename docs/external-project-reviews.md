# External emulator review ledger

`data/external-project-reviews.json` separates three facts that must not be
conflated:

- the exact revision that was substantively reviewed;
- the latest upstream revision observed by the project;
- the subsystem scope and evidence actually extracted from that review.

Run this periodically and before relying on an older external comparison:

```sh
python3 scripts/check-external-project-updates.py
```

The command is read-only. It queries each tracked repository's default-branch
HEAD and reports `review-current`, `update-available`, or
`pinned-review-needed`. `--fail-on-update` is available for scheduled checks.
It deliberately does not update the ledger: seeing a new commit is not the
same as examining its relevant changes.

After a substantive review, update `last_reviewed_on`,
`last_reviewed_revision`, `last_review_scope`, and `next_review_scope`. Also
record any useful claim in the specification/evidence ledger with a focused
test or state explicitly that no action resulted. Refresh
`latest_observed_on` and `latest_observed_revision` only when the upstream HEAD
was actually checked.

The repository-specific usage boundaries in the JSON ledger remain mandatory.
MAME's applicable BSD-3-Clause CD-i code requires attribution; MiSTer is a
clean-room behavioral/test reference; CeDImu is a behavioral comparison only.
