#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 L3Digital LLC
# SPDX-License-Identifier: GPL-3.0-only
"""Check the T25 sample streams against the spec §11.9 targets (REQ-001).

Usage: python3 docs/evidence/ms-1/performance/analyze.py [--raw DIR]

Reads the C-10 streams `run.sh` wrote, `<DIR>/t<k>-run<n>.csv` (default DIR:
`raw/` beside this file), together with `<DIR>/runs.tsv`, and prints a
Markdown summary to standard output for EV-003 to embed: per scenario and
metric the sample count, p50, p95, p99 and maximum over every run pooled,
per-run percentiles, and a pass or fail table for each target.

Exit status: 0 when every check passes; 1 when any check fails (a missed
target, fewer than the C-10 minimum runs, a run shorter than 60 s, or a run
with no sample of a metric its target needs); 2 when the input cannot be
analysed at all (no streams, or a stream that is not a C-10 stream). The
summary is still printed for status 1, so a miss is recorded with its
numbers; a status-2 report names the bad files instead.

Percentiles are nearest-rank over the raw samples, never averages (C-10):
the p-th percentile of n sorted values is the value at 1-based rank
ceil(p * n / 100). Every reported percentile is therefore an observed
sample, and a p99 over fewer than 100 samples is the maximum.

Requirements: Python 3.10 or newer with only the standard library, so the
owner can run it with the system interpreter and no environment; it reads
only files under DIR and writes only to standard output and standard error.
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

# The C-10 stream header, as `measure::recorder::HEADER` writes it; run.sh
# checks the same line. A change to the recorder's columns must change all
# three.
HEADER = ["scenario", "run", "metric", "value_ms"]
METRICS = ("cpu_usage", "first_paint", "input_to_paint", "cancel_ack")
SCENARIOS = ("T1", "T2", "T3", "T4")
PERCENTILES = (50, 95, 99)

# C-10 minimums.
MIN_RUNS = 3
MIN_SECS = 60

# Stream names as run.sh writes them: lower-case scenario, run number.
STREAM_NAME = re.compile(r"^(t[1-4])-run([1-9][0-9]*)\.csv$")

# The T23 end-to-end tests (crates/fuzzy-dedupe-app/tests/measure.rs) that
# carry the parts of §11.9 the sample stream cannot: the stream holds only
# timings, so T3's "state unchanged" and T2's copy are proven there.
T3_STATE_TEST = "measure_t3_script_opens_and_closes_every_popup_on_both_workspaces"
T2_COPY_TEST = "measure_t2_script_opens_the_5_mb_pair_toggles_wrap_and_copies_a_pane"


class InputError(Exception):
    """The raw directory does not hold analysable C-10 streams."""


@dataclass(frozen=True)
class Target:
    """One §11.9 limit on one statistic of one scenario's metric.

    `stat` is a percentile (50, 95, 99) or "max". `per_run` names a metric
    every run must sample at least once: a T2 run without `first_paint`
    never painted its diff, and a T4 run without `cancel_ack` never saw its
    cancellation acknowledged, so an absent sample is a failure, not a pass.
    """

    scenario: str
    metric: str
    stat: int | str
    limit_ms: float
    label: str
    per_run: bool = False


TARGETS = (
    Target("T1", "cpu_usage", 95, 8.0, "T1 frame CPU time p95 ≤ 8 ms"),
    Target("T1", "cpu_usage", 99, 16.7, "T1 frame CPU time p99 ≤ 16.7 ms"),
    Target("T2", "cpu_usage", 95, 8.0, "T2 frame CPU time p95 ≤ 8 ms (wrap on and off pooled)"),
    Target("T2", "cpu_usage", 99, 16.7, "T2 frame CPU time p99 ≤ 16.7 ms (wrap on and off pooled)"),
    Target("T2", "first_paint", "max", 500.0, "T2 first paint ≤ 500 ms after the diff is ready", per_run=True),
    Target("T4", "input_to_paint", 95, 100.0, "T4 input to paint p95 ≤ 100 ms"),
    Target("T4", "cancel_ack", "max", 1000.0, "T4 cancellation acknowledged ≤ 1000 ms", per_run=True),
)


def nearest_rank(sorted_values: list[float], p: int) -> float:
    """Return the nearest-rank p-th percentile of a non-empty sorted list."""
    # Integer ceiling, so 95 * n / 100 never lands one rank low through
    # floating-point rounding.
    rank = max(1, -(-p * len(sorted_values) // 100))
    return sorted_values[rank - 1]


def stat_of(sorted_values: list[float], stat: int | str) -> float:
    return sorted_values[-1] if stat == "max" else nearest_rank(sorted_values, int(stat))


def read_stream(path: Path, scenario: str, run: int) -> dict[str, list[float]]:
    """Return one stream's samples by metric.

    Raises InputError for anything that is not a C-10 stream of this
    scenario and run: a wrong header, a line naming another scenario or run,
    an unknown metric, or a value that is not a non-negative number. Such a
    stream is not pooled, because one foreign line would silently shift the
    percentiles of a scenario it does not belong to.
    """
    samples: dict[str, list[float]] = defaultdict(list)
    with path.open(newline="", encoding="utf-8") as handle:
        rows = csv.reader(handle)
        header = next(rows, None)
        if header != HEADER:
            raise InputError(f"{path.name}: header is {header!r}, not the C-10 header {','.join(HEADER)}")
        for number, row in enumerate(rows, start=2):
            if len(row) != 4:
                raise InputError(f"{path.name} line {number}: {len(row)} fields, not 4")
            row_scenario, row_run, metric, value = row
            if row_scenario != scenario or row_run != str(run):
                raise InputError(f"{path.name} line {number}: names {row_scenario} run {row_run}")
            if metric not in METRICS:
                raise InputError(f"{path.name} line {number}: unknown metric {metric!r}")
            try:
                value_ms = float(value)
            except ValueError:
                raise InputError(f"{path.name} line {number}: value {value!r} is not a number") from None
            if not value_ms >= 0.0:  # also rejects NaN
                raise InputError(f"{path.name} line {number}: value {value!r} is negative or NaN")
            samples[metric].append(value_ms)
    if not any(samples.values()):
        # C-10: a missing or empty sample stream fails the run.
        raise InputError(f"{path.name}: no sample after the header")
    return samples


def read_streams(raw: Path) -> dict[str, dict[int, dict[str, list[float]]]]:
    """Return samples as scenario -> run -> metric -> values."""
    if not raw.is_dir():
        raise InputError(f"{raw} is not a directory")
    streams: dict[str, dict[int, dict[str, list[float]]]] = defaultdict(dict)
    errors = []
    for path in sorted(raw.glob("*.csv")):
        match = STREAM_NAME.match(path.name)
        if match is None:
            errors.append(f"{path.name}: not a run.sh stream name (t<k>-run<n>.csv)")
            continue
        scenario, run = match.group(1).upper(), int(match.group(2))
        try:
            streams[scenario][run] = read_stream(path, scenario, run)
        except (InputError, UnicodeDecodeError) as err:
            errors.append(str(err))
    if errors:
        raise InputError("\n".join(errors))
    if not streams:
        raise InputError(f"no t<k>-run<n>.csv streams in {raw}")
    return streams


def read_runs(raw: Path) -> dict[tuple[str, int], tuple[int, int]] | None:
    """Return run.sh's (configured secs, wall secs) per (scenario, run).

    None when runs.tsv is absent; the C-10 length check then fails rather
    than assuming the runs were long enough.
    """
    path = raw / "runs.tsv"
    if not path.is_file():
        return None
    runs = {}
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            try:
                runs[(row["scenario"], int(row["run"]))] = (int(row["secs"]), int(row["wall_secs"]))
            except (KeyError, TypeError, ValueError):
                raise InputError(f"runs.tsv: unreadable row {row!r}") from None
    return runs


def fmt(value: float) -> str:
    return f"{value:.3f}"


def table(header: list[str], rows: list[list[str]]) -> list[str]:
    lines = ["| " + " | ".join(header) + " |", "| " + " | ".join("---" for _ in header) + " |"]
    lines += ["| " + " | ".join(row) + " |" for row in rows]
    return lines


def c10_checks(streams, runs) -> list[tuple[str, bool, str]]:
    """Return (check, passed, observed) rows for the C-10 run minimums."""
    rows = []
    for scenario in SCENARIOS:
        found = sorted(streams.get(scenario, {}))
        rows.append(
            (
                f"{scenario}: at least {MIN_RUNS} runs (C-10)",
                len(found) >= MIN_RUNS,
                f"{len(found)} runs" + (f" ({', '.join(map(str, found))})" if found else ""),
            )
        )
        if runs is None:
            rows.append((f"{scenario}: every run at least {MIN_SECS} s (C-10)", False, "runs.tsv missing"))
            continue
        short = []
        for run in found:
            secs, wall = runs.get((scenario, run), (0, 0))
            if secs < MIN_SECS or wall < secs:
                short.append(f"run {run}: {secs} s configured, {wall} s wall")
        rows.append(
            (
                f"{scenario}: every run at least {MIN_SECS} s (C-10)",
                bool(found) and not short,
                "; ".join(short) if short else ("all runs" if found else "no runs"),
            )
        )
    return rows


def target_checks(streams) -> list[tuple[str, bool, str]]:
    rows = []
    for target in TARGETS:
        by_run = streams.get(target.scenario, {})
        values = sorted(v for samples in by_run.values() for v in samples.get(target.metric, []))
        missing = [str(run) for run, samples in sorted(by_run.items()) if not samples.get(target.metric)]
        if not values:
            rows.append((target.label, False, f"no {target.metric} samples"))
            continue
        observed = stat_of(values, target.stat)
        passed = observed <= target.limit_ms
        stat = "max" if target.stat == "max" else f"p{target.stat}"
        note = f"{stat} {fmt(observed)} ms over {len(values)} samples"
        if target.per_run and missing:
            passed = False
            note += f"; no {target.metric} sample in run {', '.join(missing)}"
        rows.append((target.label, passed, note))
    return rows


def min_state_share(pooled: list[float], p95_limit: float, p99_limit: float) -> int | None:
    """Return the fewest samples a T2 wrap state must hold to be proven within budget.

    The bound needs no assumption about how samples split between wrap
    states. For a state of m of the n pooled samples, its nearest-rank p95
    sits at state rank ceil(0.95 m), so ceil(0.05 m) state samples, at
    least, are at or above it; those occupy the top of the pooled order,
    hence the state's p95 is at most the pooled sample at rank
    n - floor(0.05 m), which is the pooled nearest-rank quantile at level
    1 - 0.05 s for s = m / n. p99 likewise, at rank n - floor(0.01 m).

    The returned m is the smallest state size for which both bounds meet
    the limits; any state at least that large meets them. The bound only
    tightens as m grows, so a scan from 1 stops at the first fit. Returns
    None when even m = n fails, which is the pooled check failing.
    """
    n = len(pooled)
    for m in range(1, n + 1):
        # Integer floors, so 5 * m / 100 never rounds across a rank.
        if pooled[n - (5 * m) // 100 - 1] <= p95_limit and pooled[n - m // 100 - 1] <= p99_limit:
            return m
    return None


def wrap_bound_line(streams) -> str:
    """Return the supplementary T2 per-wrap-state line; it never decides the exit status."""
    pooled = sorted(v for samples in streams.get("T2", {}).values() for v in samples.get("cpu_usage", []))
    prefix = "- **Supplementary, T2 per wrap state (not a target check; the exit status ignores it).** "
    if not pooled:
        return prefix + "No T2 `cpu_usage` samples, so no bound."
    m = min_state_share(pooled, 8.0, 16.7)
    if m is None:
        return prefix + (
            "The pooled check already fails (s = 1 exceeds the frame budget), so no wrap state can be "
            "shown to meet p95 ≤ 8 ms and p99 ≤ 16.7 ms."
        )
    share = m / len(pooled)
    return prefix + (
        f"Each wrap state meets the T1 and T2 frame budget (p95 ≤ 8 ms, p99 ≤ 16.7 ms) if it holds at "
        f"least s* = {share:.4f} of T2's {len(pooled)} frames ({m} samples); the bound is nearest-rank on "
        "the pooled samples and assumes nothing about the split. The script spends equal wall time in "
        "each state (4 s alternation), so a state holds about half the frames unless its frame rate differs."
    )


def summary(streams, runs) -> tuple[list[str], bool]:
    """Return the Markdown summary lines and whether every check passed."""
    out = ["## T25 measurement summary", ""]
    out += [
        (
            "Percentiles are nearest-rank over the raw samples of every run of a scenario pooled: "
            "the p-th percentile of n sorted samples is the sample at 1-based rank ceil(p × n / 100). "
            "No value below is an average (C-10). All values are milliseconds."
        ),
        "",
        "### Pooled statistics",
        "",
    ]
    rows = []
    for scenario in SCENARIOS:
        by_run = streams.get(scenario, {})
        for metric in METRICS:
            values = sorted(v for samples in by_run.values() for v in samples.get(metric, []))
            if values:
                pcts = [fmt(nearest_rank(values, p)) for p in PERCENTILES]
                rows.append([scenario, metric, str(len(by_run)), str(len(values)), *pcts, fmt(values[-1])])
    out += table(["Scenario", "Metric", "Runs", "Count", "p50", "p95", "p99", "Max"], rows)

    out += ["", "### Per run", ""]
    rows = []
    for scenario in SCENARIOS:
        for run, samples in sorted(streams.get(scenario, {}).items()):
            for metric in METRICS:
                values = sorted(samples.get(metric, []))
                if values:
                    pcts = [fmt(nearest_rank(values, p)) for p in PERCENTILES]
                    rows.append([scenario, str(run), metric, str(len(values)), *pcts, fmt(values[-1])])
    out += table(["Scenario", "Run", "Metric", "Count", "p50", "p95", "p99", "Max"], rows)

    checks = c10_checks(streams, runs) + target_checks(streams)
    out += ["", "### Targets (spec §11.9) and run minimums (C-10)", ""]
    out += table(
        ["Check", "Result", "Observed"],
        [[label, "pass" if ok else "**FAIL**", observed] for label, ok, observed in checks],
    )
    all_passed = all(ok for _, ok, _ in checks)

    t3 = streams.get("T3", {})
    t3_counts = ", ".join(
        f"run {run}: {sum(len(v) for v in samples.values())} samples" for run, samples in sorted(t3.items())
    )
    out += [
        "",
        "### Notes",
        "",
        wrap_bound_line(streams),
        (
            "- **T2 wrap on and off.** The C-10 stream has no time or wrap column, so the samples "
            "cannot be split by wrap state. The T2 script starts with wrap off and toggles it with "
            "Alt+Z at every 4 s phase boundary of script time (off, then on from 4 s, off from 8 s, "
            "and so on), so both states alternate in 4 s blocks through each run. `cpu_usage` is "
            "the CPU time of one frame, not its wall time, so the sample order cannot be mapped "
            "back onto script time either. The T2 `cpu_usage` rows above therefore pool both wrap "
            "states; a per-state percentile needs the stream to record the wrap state or a "
            "timestamp, which it does not."
        ),
        (
            "- **T2 copy.** The stream carries timings only. That the copy equals the selected "
            "text (a whole pane after Ctrl+A) is proven by the T23 end-to-end test "
            f"`{T2_COPY_TEST}`, not by these samples."
        ),
        (
            f"- **T3 state unchanged.** T3 has no numeric target; its samples ({t3_counts or 'none'}) "
            "are reported for completeness. That every popup on both workspaces leaves the "
            f"workspace state unchanged is proven by the T23 end-to-end test `{T3_STATE_TEST}`, "
            "together with every T3 run completing with a non-empty stream."
        ),
        (
            "- **Known bias of `first_paint`.** The sample starts when the app drains the "
            "comparison event, not when the engine finishes the diff, so it omits at most one "
            "frame interval between the two. The true first paint can exceed the recorded value "
            "by up to one frame interval."
        ),
    ]
    out += ["", f"Overall: **{'PASS' if all_passed else 'FAIL'}**"]
    return out, all_passed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--raw",
        type=Path,
        default=Path(__file__).resolve().parent / "raw",
        help="directory holding run.sh's streams and runs.tsv (default: raw/ beside this file)",
    )
    args = parser.parse_args(argv)
    try:
        streams = read_streams(args.raw)
        runs = read_runs(args.raw)
    except InputError as err:
        print(f"analyze.py: cannot analyse {args.raw}:\n{err}", file=sys.stderr)
        return 2
    lines, passed = summary(streams, runs)
    print("\n".join(lines))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
