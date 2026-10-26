#!/usr/bin/env bash
# Fails if tracked files contain game data or obvious decompiler output.
# See docs/CLEAN_ROOM.md. This is a grep, not a proof.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
fail=0

if git ls-files -z | grep -zEi '\.(esm|esp|bsa|nif|kf|dds|exe|dll)$' | tr '\0' '\n' | grep .; then
    echo "error: game files are tracked (above)" >&2
    fail=1
fi

if git grep -nIE '\b(FUN|DAT|LAB|sub|loc|off|unk)_[0-9a-fA-F]{6,8}\b' -- ':!scripts/publish-check.sh' ':!docs/CLEAN_ROOM.md'; then
    echo "error: decompiler placeholder names found (above)" >&2
    fail=1
fi

# Retail Oblivion.exe images load at 0x00400000; flag literal addresses in that range.
if git grep -nIE '0x00[4-9a-fA-F][0-9a-fA-F]{5}\b' -- '*.rs' 'docs/**'; then
    echo "error: what look like retail addresses found (above)" >&2
    fail=1
fi

exit $fail
