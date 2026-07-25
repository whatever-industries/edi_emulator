#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Reproducibly extract/OCR CD-i PDF references into ignored local storage.
set -eu

source_root=${1:-"/Volumes/Projects/Coding/disc specs/Philips CD-i - icdia-site-documents-2026-07-18"}
output_root=${2:-"references/spec-text"}

if [ "$source_root" = "--help" ] || [ "$source_root" = "-h" ]; then
    echo "usage: scripts/ocr-cdi-specs.sh [SOURCE_ROOT] [OUTPUT_ROOT]"
    exit 0
fi

for tool in pdftotext pdfinfo shasum; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "missing required tool: $tool" >&2
        exit 1
    }
done

mkdir -p "$output_root"
manifest="$output_root/manifest.tsv"
tmp_manifest="$output_root/manifest.tsv.new"
: >"$tmp_manifest"
printf 'sha256\tpages\tmethod\tsource\ttext\n' >>"$tmp_manifest"

find "$source_root" -type f -iname '*.pdf' -print0 |
while IFS= read -r -d '' pdf; do
    relative=${pdf#"$source_root"/}
    stem=${relative%.pdf}
    text_path="$output_root/$stem.txt"
    mkdir -p "$(dirname "$text_path")"
    pages=$(pdfinfo "$pdf" | awk '/^Pages:/ { print $2; exit }')
    if [ -f "$text_path" ]; then
        visible=$(tr -d '[:space:]' <"$text_path" | wc -c | tr -d ' ')
    else
        visible=0
    fi
    if [ "$visible" -ge "$((pages * 40))" ]; then
        method=reused-local
    else
        pdftotext -layout "$pdf" "$text_path"
        method=pdftotext
        visible=$(tr -d '[:space:]' <"$text_path" | wc -c | tr -d ' ')
    fi
    if [ "$visible" -lt "$((pages * 40))" ]; then
        for tool in pdftoppm tesseract; do
            command -v "$tool" >/dev/null 2>&1 || {
                echo "$relative needs OCR but $tool is unavailable" >&2
                exit 1
            }
        done
        page_dir="$output_root/.page"
        mkdir -p "$page_dir"
        ocr_path="$text_path.ocr-new"
        : >"$ocr_path"
        page_number=1
        while [ "$page_number" -le "$pages" ]; do
            pdftoppm -f "$page_number" -l "$page_number" -r 240 -gray \
                -singlefile -png "$pdf" "$page_dir/page" >/dev/null 2>&1
            tesseract "$page_dir/page.png" stdout --dpi 240 2>/dev/null >>"$ocr_path"
            printf '\n\f\n' >>"$ocr_path"
            rm -f "$page_dir/page.png"
            page_number=$((page_number + 1))
        done
        rmdir "$page_dir"
        mv "$ocr_path" "$text_path"
        method=tesseract-240dpi
    fi
    digest=$(shasum -a 256 "$pdf" | awk '{ print $1 }')
    printf '%s\t%s\t%s\t%s\t%s\n' \
        "$digest" "$pages" "$method" "$relative" "${stem}.txt" >>"$tmp_manifest"
done

mv "$tmp_manifest" "$manifest"
echo "$manifest"
