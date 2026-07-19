#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Fetch the SingleStepTests m68000 vectors (MIT-licensed) used by the
# cdi-scc68070 conformance tests. Files land in tests-data/harte-68000/.
set -eu

dest="$(dirname "$0")/../tests-data/harte-68000"
mkdir -p "$dest"
base="https://raw.githubusercontent.com/SingleStepTests/m68000/main/v1"

files=$(curl -s "https://api.github.com/repos/SingleStepTests/m68000/contents/v1" \
    | grep -o '"name": *"[^"]*\.json\.bin"' | sed 's/.*"\([^"]*\)"$/\1/')

for f in $files; do
    if [ ! -f "$dest/$f" ]; then
        echo "fetch $f"
        curl -sL -o "$dest/$f" "$base/$f"
    fi
done
echo "done: $(ls "$dest" | wc -l | tr -d ' ') files in $dest"
