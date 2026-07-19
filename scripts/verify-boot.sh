#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Deterministic boot regression check: boots cdi220b.rom headlessly and
# compares the framebuffer hash against tests-data/hashes.toml.
set -eu

cd "$(dirname "$0")/.."
expected=$(grep '^sha256' tests-data/hashes.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
actual=$(cargo run -q -p cdi-cli --release -- boot roms/cdi220b.rom \
    --instructions 150000000 --hash | grep 'SHA-256' | awk '{print $3}')

if [ "$expected" = "$actual" ]; then
    echo "boot hash OK: $actual"
else
    echo "boot hash MISMATCH: expected $expected got $actual" >&2
    exit 1
fi
