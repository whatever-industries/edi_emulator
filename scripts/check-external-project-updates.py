#!/usr/bin/env python3
"""Report whether tracked external emulator projects moved since review."""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "data" / "external-project-reviews.json"


def fetch_head(repository: str, branch: str) -> str:
    url = f"https://api.github.com/repos/{repository}/commits/{branch}"
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "E-Di-external-project-review-checker",
        },
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        payload = json.load(response)
    return str(payload["sha"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--fail-on-update",
        action="store_true",
        help="exit with status 1 when a project needs review",
    )
    args = parser.parse_args()

    with LEDGER.open(encoding="utf-8") as handle:
        ledger = json.load(handle)

    update_available = False
    print("project\treviewed\tupstream\tstatus")
    for project in ledger["projects"]:
        reviewed = project["last_reviewed_revision"]
        try:
            upstream = fetch_head(project["repository"], project["default_branch"])
        except (OSError, urllib.error.URLError, KeyError, json.JSONDecodeError) as error:
            print(
                f"{project['name']}\t{reviewed or '-'}\t-\tcheck-failed: {error}",
                file=sys.stderr,
            )
            continue

        if reviewed is None:
            status = "pinned-review-needed"
            update_available = True
        elif upstream == reviewed:
            status = "review-current"
        else:
            status = "update-available"
            update_available = True

        print(f"{project['name']}\t{reviewed or '-'}\t{upstream}\t{status}")

    return int(args.fail_on_update and update_available)


if __name__ == "__main__":
    raise SystemExit(main())
