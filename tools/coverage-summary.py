#!/usr/bin/env python3
"""Render an LCOV report as a GitHub Actions job-summary Markdown table.

The Docker build steps already write to $GITHUB_STEP_SUMMARY; this adds the
coverage totals next to them so a reviewer does not have to download the HTML
artifact to see how much of the workspace was exercised.
"""

from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path


def crate_of(source: str) -> str:
    """Group LCOV `SF:` paths by workspace crate name."""
    marker = "/crates/"
    if marker in source:
        return source.split(marker, 1)[1].split("/", 1)[0]
    return "other"


def percent(hit: int, found: int) -> float:
    return 100.0 * hit / found if found else 0.0


def main() -> int:
    report = Path(sys.argv[1] if len(sys.argv) > 1 else "coverage/lcov.info")
    if not report.is_file():
        print(
            f":warning: No coverage report at `{report}`; run a coverage suite first.",
            file=sys.stderr,
        )
        return 1

    per_crate: dict[str, list[int]] = defaultdict(lambda: [0, 0])  # [hit, found]
    total_hit = total_found = 0
    source = ""
    for line in report.read_text(errors="replace").splitlines():
        if line.startswith("SF:"):
            source = line[3:].strip()
        elif line.startswith("LH:"):
            hit = int(line[3:])
            per_crate[crate_of(source)][0] += hit
            total_hit += hit
        elif line.startswith("LF:"):
            found = int(line[3:])
            per_crate[crate_of(source)][1] += found
            total_found += found

    print("### Coverage")
    print()
    if total_found == 0:
        print(":warning: The coverage report contains no line data.")
        return 0

    print(f"**Lines covered:** {total_hit}/{total_found} ({percent(total_hit, total_found):.1f}%)")
    print()
    print("| Crate | Lines covered | Coverage |")
    print("| --- | ---: | ---: |")
    ranked = sorted(
        per_crate.items(),
        key=lambda entry: (-percent(entry[1][0], entry[1][1]), entry[0]),
    )
    for crate, (hit, found) in ranked:
        print(f"| `{crate}` | {hit}/{found} | {percent(hit, found):.1f}% |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
