"""Summarize KLBF timing samples for a gameplay report (Python 3).

    py -3 docs/klbf/summarize_log.py run.log --lines 1200 2500 --breakdown

Choose inclusive line numbers around one scene, or pass a log already trimmed
to that scene. The output gives medians of logged samples, not per-frame lows.
Only complete timing records contribute to the breakdown. Copy and texture
preparation are included in CPU preparation time.
"""

import argparse
import codecs
import io
import re
from pathlib import Path
from statistics import median


DECIMAL = r"\d+(?:\.\d+)?"
NUMBER = rf"({DECIMAL})"
HOST = re.compile(
    rf"{NUMBER} fps, {NUMBER} ms/frame \({NUMBER} preparing:"
)
BREAKDOWN = re.compile(
    rf"\s*copies {NUMBER} ms {DECIMAL} MB, textures {NUMBER} ms {DECIMAL} MB; "
    rf"{NUMBER} on the GPU; {NUMBER} copying back {DECIMAL} MB\), "
    rf"{NUMBER} draws/frame, dropped {NUMBER} without a host pipeline"
)
FRAMEGEN = re.compile(
    rf"ffx: frame generation: {NUMBER} real fps, {NUMBER} fps presented"
)
STATE = re.compile(r"\bon=(true|false)\b")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def summarize(lines, first, last, breakdown=False):
    host = []
    stages = []
    framegen = {}
    for number, line in enumerate(lines, 1):
        if last is not None and number > last:
            break
        if number < first:
            continue
        line = ANSI.sub("", line)
        match = HOST.search(line)
        if match:
            host.append(tuple(map(float, match.groups())))
            if breakdown:
                detail = BREAKDOWN.match(line, match.end())
                if detail:
                    stages.append(tuple(map(float, detail.groups())))
            continue
        match = FRAMEGEN.search(line)
        if match:
            state = STATE.search(line)
            key = state[1] if state else "unspecified"
            framegen.setdefault(key, []).append(tuple(map(float, match.groups())))

    report = []
    if host:
        fps, ms, cpu = map(median, zip(*host))
        report.append(f"Host timing ({len(host)} samples, medians): {fps:.1f} FPS, "
                      f"{ms:.1f} ms/frame, {cpu:.1f} ms/frame preparing on CPU")
    if stages:
        copies, textures, gpu, back, draws, dropped = map(median, zip(*stages))
        report.append(f"Host breakdown (medians per frame, n={len(stages)}): "
                      f"preparing copies {copies:.1f} ms, textures {textures:.1f} ms; "
                      f"GPU {gpu:.1f} ms, copying back {back:.1f} ms; "
                      f"{draws:g} draws, {dropped:g} dropped without a host pipeline")
    for key, samples in framegen.items():
        real, presented = map(median, zip(*samples))
        report.append(f"Frame generation on={key} ({len(samples)} samples, medians): "
                      f"{real:.1f} real FPS, {presented:.1f} presented FPS")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("log", type=Path)
    parser.add_argument("--breakdown", action="store_true",
                        help="show copy, texture, GPU, copy-back and draw counters when logged")
    parser.add_argument("--lines", nargs=2, type=int, metavar=("FIRST", "LAST"),
                        help="inclusive 1-based line range for a scene")
    args = parser.parse_args()
    first, last = args.lines or (1, None)
    if first < 1 or (last is not None and last < first):
        parser.error("expected 1 <= FIRST <= LAST")
    try:
        with args.log.open("rb") as raw:
            prefix = raw.peek(2)[:2]
            encoding = "utf-16" if prefix in (codecs.BOM_UTF16_LE, codecs.BOM_UTF16_BE) else "utf-8-sig"
            with io.TextIOWrapper(raw, encoding=encoding, errors="replace") as lines:
                report = summarize(lines, first, last, args.breakdown)
    except OSError as error:
        parser.exit(1, f"{error}\n")
    if not report:
        parser.exit(1, "No KLBF timing samples found in the selected lines.\n")
    print("\n".join(report))


if __name__ == "__main__":
    main()
