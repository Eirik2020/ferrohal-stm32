"""Summarises the rig telemetry tools/rig_run.py records.

    python3 -I tools/rig_report.py [--since 7d|24h|<ISO date>] [--board <bench or board>]
                                   [--top N] [--full] [--json] [--file <path>]

The default is short, for a status answer: per board its runs, success rate,
total, mean and p95 time per run, and where that time went (build, flash,
execution); the busiest board; the slowest runs; flaky runs (pass and not pass
at one commit); retries; and how busy the rig was over the window. --full adds
the per-run-letter table and the session phases. Reads hil/logs/telemetry/
runs.jsonl, or RIG_TELEMETRY, or --file. Writes nothing.
"""

from __future__ import annotations

import datetime
import json
import math
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rig_run  # noqa: E402  (DEFAULT_LOG)

UTC = datetime.timezone.utc


def parse_time(text):
    return datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))


def since_of(text, now):
    """`7d`, `24h`, `90m`, or an ISO date or time (UTC when it names no zone)."""
    m = re.fullmatch(r"(\d+)([dhm])", text)
    if m:
        unit = {"d": "days", "h": "hours", "m": "minutes"}[m.group(2)]
        return now - datetime.timedelta(**{unit: int(m.group(1))})
    t = datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))
    return t if t.tzinfo else t.replace(tzinfo=UTC)


def load(path):
    """Every well-formed record; a torn or foreign line is skipped."""
    out = []
    try:
        with open(path) as f:
            for line in f:
                try:
                    r = json.loads(line)
                    r["_start"] = parse_time(r["start"])
                    r["_end"] = parse_time(r["end"])
                except (ValueError, KeyError, TypeError, AttributeError):
                    continue
                out.append(r)
    except FileNotFoundError:
        pass
    return out


def p95(values):
    """Nearest-rank 95th percentile."""
    if not values:
        return 0.0
    s = sorted(values)
    return s[max(0, math.ceil(0.95 * len(s)) - 1)]


def dur(sec):
    sec = int(round(sec))
    if sec < 60:
        return f"{sec}s"
    if sec < 3600:
        return f"{sec // 60}m{sec % 60:02d}s"
    if sec < 86400:
        return f"{sec // 3600}h{sec % 3600 // 60:02d}m"
    return f"{sec // 86400}d{sec % 86400 // 3600:02d}h"


def busy(sessions, start, end):
    """(busy seconds, longest idle gap seconds) of the merged session intervals
    inside [start, end]."""
    spans = sorted((max(s["_start"], start), min(s["_end"], end)) for s in sessions)
    spans = [(a, b) for a, b in spans if b > a]
    merged = []
    for a, b in spans:
        if merged and a <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], b))
        else:
            merged.append((a, b))
    total = sum((b - a).total_seconds() for a, b in merged)
    edges = [start] + [t for span in merged for t in span] + [end]
    gaps = [(edges[i + 1] - edges[i]).total_seconds() for i in range(0, len(edges), 2)]
    return total, max(gaps) if gaps else 0.0


def summarise(records, start, end, board=None, top=5):
    """Everything the report prints, as plain data (also the --json output)."""
    def wanted(r):
        return start <= r["_start"] <= end and (board is None or board in (r.get("bench"), r.get("board")))

    runs = [r for r in records if r.get("kind") == "run" and wanted(r)]
    sessions = [r for r in records if r.get("kind") == "session" and wanted(r)]

    boards = {}
    for name in sorted({r["board"] for r in runs} | {s["board"] for s in sessions}):
        rs = [r for r in runs if r["board"] == name]
        ss = [s for s in sessions if s["board"] == name]
        walls = [r["wall_s"] for r in rs]
        phase = defaultdict(float)
        for r in rs:
            for k, v in r.get("phases", {}).items():
                phase[k] += v
        # A session with no run lines (a flashed bench) still counts its time.
        if not rs:
            walls = [s["wall_s"] for s in ss]
            for s in ss:
                phase["build"] += s.get("phases", {}).get("build", 0.0)
            phase["flash+exec"] = sum(walls) - phase["build"]
        for s in ss:
            for k in ("probe_select", "teardown"):
                phase[k] += s.get("phases", {}).get(k, 0.0)
            if rs:
                phase["build"] += s.get("phases", {}).get("build", 0.0)
        results = [r["result"] for r in (rs or ss)]
        boards[name] = {
            "sessions": len(ss),
            "runs": len(rs),
            "passed": results.count("pass"),
            "results": {k: results.count(k) for k in sorted(set(results))},
            "total_s": round(sum(s["wall_s"] for s in ss) or sum(walls), 1),
            "mean_s": round(sum(walls) / len(walls), 1) if walls else 0.0,
            "p95_s": round(p95(walls), 1),
            "phases_s": {k: round(v, 1) for k, v in phase.items() if v},
        }

    by_letter = defaultdict(list)
    for r in runs:
        by_letter[(r["bench"], r["run"])].append(r)
    letters = {
        f"{b} {x}": {"runs": len(rs), "passed": sum(r["result"] == "pass" for r in rs),
                     "mean_s": round(sum(r["wall_s"] for r in rs) / len(rs), 1),
                     "p95_s": round(p95([r["wall_s"] for r in rs]), 1)}
        for (b, x), rs in sorted(by_letter.items())
    }

    by_commit = defaultdict(set)
    for r in runs:
        by_commit[(r["bench"], r["run"], r.get("commit"))].add(r["result"])
    flaky = sorted(f"{b} {x} at {c}: {', '.join(sorted(res))}"
                   for (b, x, c), res in by_commit.items() if "pass" in res and len(res) > 1)

    slowest = sorted(runs, key=lambda r: -r["wall_s"])[:top]
    retries = [r for r in runs if r.get("attempt", 1) > 1]
    busy_s, idle_gap = busy(sessions, start, end)
    window = (end - start).total_seconds()
    return {
        "window": {"start": start.isoformat(timespec="minutes"), "end": end.isoformat(timespec="minutes"),
                   "seconds": round(window)},
        "sessions": len(sessions),
        "runs": len(runs),
        "busy_s": round(busy_s),
        "busy_pct": round(100 * busy_s / window, 1) if window else 0.0,
        "longest_idle_s": round(idle_gap),
        "boards": boards,
        "run_letters": letters,
        "slowest": [{"run": f"{r['bench']} {r['run']}", "wall_s": r["wall_s"], "start": r["start"],
                     "result": r["result"], "phases_s": r.get("phases", {})} for r in slowest],
        "flaky": flaky,
        "retries": len(retries),
        "not_pass": [{"run": f"{r['bench']} {r['run']}", "result": r["result"], "start": r["start"],
                      "commit": r.get("commit"), "requester": r.get("requester")}
                     for r in runs if r["result"] != "pass"][-top:],
    }


def share(phases, total):
    if not total:
        return ""
    big = sorted(phases.items(), key=lambda kv: -kv[1])[:3]
    return ", ".join(f"{k} {round(100 * v / total)}%" for k, v in big)


def render(s, full=False):
    out = []
    w = s["window"]
    out.append(f"Rig, {w['start'][:16].replace('T', ' ')} to {w['end'][:16].replace('T', ' ')} UTC: "
               f"{s['sessions']} sessions, {s['runs']} runs; busy {dur(s['busy_s'])} "
               f"({s['busy_pct']}%), longest idle {dur(s['longest_idle_s'])}")
    if not s["boards"]:
        out.append("No runs recorded in this window (record with tools/rig_run.py).")
        return "\n".join(out)
    out.append(f"{'board':<18} {'runs':>4} {'pass':>5} {'total':>7} {'mean':>7} {'p95':>7}  where the time went")
    for name, b in sorted(s["boards"].items(), key=lambda kv: -kv[1]["total_s"]):
        n = b["runs"] or b["sessions"]
        rate = f"{round(100 * b['passed'] / n)}%" if n else "-"
        phases_total = sum(b["phases_s"].values())
        out.append(f"{name:<18} {n:>4} {rate:>5} {dur(b['total_s']):>7} {dur(b['mean_s']):>7} "
                   f"{dur(b['p95_s']):>7}  {share(b['phases_s'], phases_total)}")
    if full:
        out.append("")
        out.append(f"{'run':<16} {'runs':>4} {'pass':>5} {'mean':>7} {'p95':>7}")
        for name, x in s["run_letters"].items():
            out.append(f"{name:<16} {x['runs']:>4} {round(100 * x['passed'] / x['runs']):>4}% "
                       f"{dur(x['mean_s']):>7} {dur(x['p95_s']):>7}")
    if s["slowest"]:
        out.append("Slowest: " + "; ".join(
            f"{r['run']} {dur(r['wall_s'])}" + (f" ({max(r['phases_s'], key=r['phases_s'].get)})" if r["phases_s"] else "")
            for r in s["slowest"]))
    out.append(f"Flaky: {'; '.join(s['flaky']) if s['flaky'] else 'none'}. Retries: {s['retries']}.")
    if s["not_pass"]:
        out.append("Latest not passing: " + "; ".join(
            f"{r['run']} {r['result']} {r['start'][5:16].replace('T', ' ')}" for r in s["not_pass"]))
    return "\n".join(out)


def main(argv):
    args = list(argv)

    def opt(name, default=None):
        if name in args:
            i = args.index(name)
            if i + 1 >= len(args):
                sys.exit(f"rig_report: {name} needs a value")
            value = args[i + 1]
            del args[i:i + 2]
            return value
        return default

    if "-h" in args or "--help" in args:
        print(__doc__.strip())
        return 0
    path = opt("--file") or os.environ.get("RIG_TELEMETRY") or rig_run.DEFAULT_LOG
    now = datetime.datetime.now(UTC)
    try:
        start = since_of(opt("--since", "7d"), now)
    except ValueError as e:
        sys.exit(f"rig_report: --since: {e}")
    board = opt("--board")
    top = int(opt("--top", "5"))
    as_json, full = "--json" in args, "--full" in args
    s = summarise(load(path), start, now, board, top)
    print(json.dumps(s, indent=1) if as_json else render(s, full))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
