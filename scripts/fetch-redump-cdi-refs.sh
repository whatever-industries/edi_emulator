#!/bin/sh
# Fetch the current official Redump Philips CD-i DAT into ignored references.
# The DAT contains metadata and hashes only; no disc media is downloaded.
set -eu

url='https://redump.info/datfile/CDI/serial%2Cversion'
destination=${1:-references/redump-cdi}
archive="$destination/redump-cdi-serial-version.zip"

mkdir -p "$destination"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/edi-redump.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

curl -fL --retry 3 --output "$temporary/redump-cdi.zip" "$url"
cp "$temporary/redump-cdi.zip" "$archive"
unzip -oq "$archive" -d "$destination"

(
    cd "$destination"
    shasum -a 256 redump-cdi-serial-version.zip ./*.dat > SHA256SUMS
)

printf '%s\n' \
    'Official source: https://redump.info/downloads' \
    "DAT endpoint: $url" \
    "Retrieved (UTC): $(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    > "$destination/SOURCE.txt"

printf 'Redump CD-i reference metadata written to %s\n' "$destination"
