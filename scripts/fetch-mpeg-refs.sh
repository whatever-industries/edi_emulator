#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
reference_root="$repo_root/references/mpeg"
mister_dir="$reference_root/CDi_MiSTer"
mister_commit=bbaf100b5b7ab02af3f5932492c4989d5f91323f
mpeg_decoder_dir="$reference_root/gen2brain-mpeg"
mpeg_decoder_commit=27c6f084c6ca342380c99a59a6a130b3f716e9d7
snapshot_date=2026-07-19

mkdir -p "$reference_root/cdiemu"

if [[ ! -d "$mister_dir/.git" ]]; then
    git clone --filter=blob:none https://github.com/MiSTer-devel/CDi_MiSTer.git "$mister_dir"
elif [[ -n $(git -C "$mister_dir" status --porcelain) ]]; then
    echo "refusing to change dirty reference checkout: $mister_dir" >&2
    exit 1
fi

git -C "$mister_dir" fetch origin "$mister_commit"
git -C "$mister_dir" checkout --detach "$mister_commit"

if [[ ! -d "$mpeg_decoder_dir/.git" ]]; then
    git clone --filter=blob:none https://github.com/gen2brain/mpeg.git "$mpeg_decoder_dir"
elif [[ -n $(git -C "$mpeg_decoder_dir" status --porcelain) ]]; then
    echo "refusing to change dirty reference checkout: $mpeg_decoder_dir" >&2
    exit 1
fi

git -C "$mpeg_decoder_dir" fetch origin "$mpeg_decoder_commit"
git -C "$mpeg_decoder_dir" checkout --detach "$mpeg_decoder_commit"

curl --fail --location --silent --show-error \
    https://www.cdiemu.org/site/cditypes.htm \
    --output "$reference_root/cdiemu/cditypes-$snapshot_date.html"
curl --fail --location --silent --show-error \
    https://www.cdiemu.org/site/dvcarts.htm \
    --output "$reference_root/cdiemu/dvc-support-$snapshot_date.html"

inventory="$reference_root/cdiemu/local-cdiemu-inventory-$snapshot_date.txt"
if [[ -d "$repo_root/references/cdiemu-v053b9" ]]; then
    find "$repo_root/references/cdiemu-v053b9" -type f -print \
        | sed "s|$repo_root/||" \
        | LC_ALL=C sort > "$inventory"
else
    : > "$inventory"
fi

(
    cd "$reference_root"
    find cdiemu -type f ! -name SOURCES.sha256 -print0 \
        | LC_ALL=C sort -z \
        | xargs -0 shasum -a 256
) > "$reference_root/SOURCES.sha256"

echo "MPEG references ready in $reference_root"
echo "MiSTer commit: $(git -C "$mister_dir" rev-parse HEAD)"
echo "gen2brain/mpeg commit: $(git -C "$mpeg_decoder_dir" rev-parse HEAD)"
cat "$reference_root/SOURCES.sha256"
