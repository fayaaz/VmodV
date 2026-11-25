#!/usr/bin/env bash
# Print the ELF machine of FILE as x86_64, aarch64 or unknown.
#
# Std-only header reader shared by tools/vmodv-deb.sh and
# tools/vmodv-deb-check.sh: it needs neither readelf, file nor a runnable
# binary, just the first 20 bytes of the ELF header (magic at offset 0,
# e_machine at offset 18, little-endian). Missing or non-ELF input exits
# non-zero so callers can treat "no machine" as a hard failure.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: tools/elf-machine.sh FILE" >&2
    exit 2
fi

FILE=$1
if [ ! -f "$FILE" ]; then
    echo "elf-machine: no such file: $FILE" >&2
    exit 1
fi

# od emits 16 bytes per line; flatten so e_machine spans line boundaries.
read -r -a HEADER <<<"$(od -An -v -tx1 -N20 -- "$FILE" 2>/dev/null | tr '\n' ' ')" || true

if [ "${#HEADER[@]}" -lt 20 ]; then
    echo "elf-machine: truncated ELF header: $FILE" >&2
    exit 1
fi
if [ "${HEADER[0]}${HEADER[1]}${HEADER[2]}${HEADER[3]}" != "7f454c46" ]; then
    echo "elf-machine: not an ELF file: $FILE" >&2
    exit 1
fi

case "${HEADER[18]}${HEADER[19]}" in
    3e00) echo "x86_64" ;;
    b700) echo "aarch64" ;;
    *) echo "unknown" ;;
esac
