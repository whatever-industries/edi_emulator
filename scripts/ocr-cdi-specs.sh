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

if [ ! -d "$source_root" ]; then
    echo "CD-i reference root is not a directory: $source_root" >&2
    exit 2
fi

pdf_count=$(find "$source_root" -type f -iname '*.pdf' -print | awk 'END { print NR + 0 }')
if [ "$pdf_count" -eq 0 ]; then
    echo "CD-i reference root contains no PDF files: $source_root" >&2
    exit 2
fi

mkdir -p "$output_root"
manifest="$output_root/manifest.tsv"
tmp_manifest="$output_root/manifest.tsv.new"
: >"$tmp_manifest"
printf 'sha256\tpages\tmethod\tsource\ttext\n' >>"$tmp_manifest"

text_is_useful() {
    candidate=$1
    page_count=$2
    [ -f "$candidate" ] || return 1

    visible=$(tr -d '[:space:]' <"$candidate" | wc -c | tr -d ' ')
    [ "$visible" -ge "$((page_count * 40))" ] || return 1

    # Some scans expose only a repeated digitizer watermark to pdftotext.
    # Character count alone makes those files look searchable. Require a
    # modest number of distinct, non-empty lines as well.
    distinct_lines=$(
        awk '
            {
                line = $0
                gsub(/[[:space:]]+/, " ", line)
                sub(/^ /, "", line)
                sub(/ $/, "", line)
                if (length(line) >= 4 && !seen[line]++) {
                    count++
                }
            }
            END { print count + 0 }
        ' "$candidate"
    )
    minimum_lines=$((page_count * 2))
    [ "$minimum_lines" -ge 12 ] || minimum_lines=12
    [ "$distinct_lines" -ge "$minimum_lines" ]
}

find "$source_root" -type f -iname '*.pdf' -print0 |
while IFS= read -r -d '' pdf; do
    relative=${pdf#"$source_root"/}
    stem=${relative%.pdf}
    text_path="$output_root/$stem.txt"
    mkdir -p "$(dirname "$text_path")"
    pages=$(pdfinfo "$pdf" | awk '/^Pages:/ { print $2; exit }')
    printf 'extracting %s (%s pages)\n' "$relative" "$pages" >&2
    if text_is_useful "$text_path" "$pages"; then
        method=reused-local
    else
        pdftotext -layout "$pdf" "$text_path"
        method=pdftotext
    fi
    if ! text_is_useful "$text_path" "$pages"; then
        for tool in pdftoppm tesseract; do
            command -v "$tool" >/dev/null 2>&1 || {
                echo "$relative needs OCR but $tool is unavailable" >&2
                exit 1
            }
        done
        page_dir="$output_root/.page.$$"
        mkdir -p "$page_dir"
        ocr_path="$text_path.ocr-new"
        : >"$ocr_path"
        page_number=1
        while [ "$page_number" -le "$pages" ]; do
            printf '  OCR page %s/%s\r' "$page_number" "$pages" >&2
            pdftoppm -f "$page_number" -l "$page_number" -r 240 -gray \
                -singlefile -png "$pdf" "$page_dir/page" >/dev/null 2>&1
            tesseract "$page_dir/page.png" stdout --dpi 240 2>/dev/null >>"$ocr_path"
            printf '\n\f\n' >>"$ocr_path"
            rm -f "$page_dir/page.png"
            page_number=$((page_number + 1))
        done
        printf '\n' >&2
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
