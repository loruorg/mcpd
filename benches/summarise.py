#!/usr/bin/env python3
"""Turn results.jsonl into a table a person can read.

Every row reports the spread across repeats, never a single sample. One sample
on a machine that is shared with anything else is not a measurement, and the
spread is the part that says whether the middle number can be trusted.
"""

import json
import sys
from collections import defaultdict


def load(path):
    rows = []
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def spread(values):
    """min, median, max, and the max/min ratio, formatted."""
    if not values:
        return "-", "-", "-", "-"
    vs = sorted(values)
    mid = vs[len(vs) // 2] if len(vs) % 2 else (vs[len(vs) // 2 - 1] + vs[len(vs) // 2]) / 2
    ratio = vs[-1] / vs[0] if vs[0] else float("inf")
    return vs[0], mid, vs[-1], ratio


def fnum(v, places=1):
    if isinstance(v, str):
        return v
    return f"{v:,.{places}f}"


def table(title, headers, rows, note=None):
    print()
    print(title)
    print("-" * len(title))
    if note:
        print(note)
    widths = [len(h) for h in headers]
    cells = [[str(c) for c in r] for r in rows]
    for r in cells:
        for i, c in enumerate(r):
            widths[i] = max(widths[i], len(c))
    print("  ".join(h.ljust(widths[i]) for i, h in enumerate(headers)))
    print("  ".join("-" * w for w in widths))
    for r in cells:
        print("  ".join(c.ljust(widths[i]) for i, c in enumerate(r)))


def main(path):
    rows = load(path)

    # ------------------------------------------------------------- sweep
    sweep = defaultdict(list)
    for r in rows:
        if r.get("phase") == "sweep":
            sweep[(r["redact"], r["batch"], r["conns"])].append(r)

    out = []
    for key in sorted(sweep, key=lambda k: (k[0] != "on", k[2], k[1])):
        redact, batch, conns = key
        rs = sweep[key]
        lo, mid, hi, ratio = spread([x["spans_per_sec"] for x in rs])
        _, p50, _, _ = spread([x["p50_ms"] for x in rs])
        _, p99, _, _ = spread([x["p99_ms"] for x in rs])
        errs = sum(x["errors"] for x in rs)
        out.append([
            redact, batch, conns, len(rs),
            fnum(mid), f"{fnum(lo)} .. {fnum(hi)}", f"{ratio:.2f}x",
            fnum(p50, 2), fnum(p99, 2), errs,
        ])
    if out:
        table(
            "Ingest: spans per second, fixed work into an empty store",
            ["redact", "batch", "conns", "reps", "spans/s med", "spans/s range",
             "spread", "p50 ms", "p99 ms", "errors"],
            out,
            note="Latency is per HTTP request, so a batch of 1000 spans is one sample.",
        )

    # ---------------------------------------------------- redaction price
    price = []
    on = {(k[1], k[2]): v for k, v in sweep.items() if k[0] == "on"}
    off = {(k[1], k[2]): v for k, v in sweep.items() if k[0] == "off"}
    for k in sorted(set(on) & set(off), key=lambda k: (k[1], k[0])):
        _, a, _, _ = spread([x["spans_per_sec"] for x in on[k]])
        _, b, _, _ = spread([x["spans_per_sec"] for x in off[k]])
        delta = (b - a) / a * 100 if a else 0
        price.append([k[0], k[1], fnum(a), fnum(b), f"{delta:+.1f}%"])
    if price:
        table(
            "The price of redaction",
            ["batch", "conns", "on (spans/s)", "off (spans/s)", "gain from --no-redact"],
            price,
            note="Redaction is on by default. Positive means turning it off was faster.",
        )

    # ------------------------------------------------------------- decay
    decay = [r for r in rows if r.get("phase") == "decay"]
    if decay:
        out = []
        for r in sorted(decay, key=lambda r: r["window"]):
            per_span = r["p50_ms"] / r["batch"]
            out.append([
                r["window"], f"{r['rows_before']:,}", f"{r['rows_after']:,}",
                fnum(r["spans_per_sec"]), fnum(r["p50_ms"], 2), fnum(per_span, 3),
            ])
        first = decay[0]["spans_per_sec"]
        last = sorted(decay, key=lambda r: r["window"])[-1]["spans_per_sec"]
        table(
            "Ingest against one store as it fills",
            ["window", "rows before", "rows after", "spans/s", "p50 ms", "p50 ms per span"],
            out,
            note=(f"Same daemon throughout, batch 10, one connection. "
                  f"First window {first:,.0f} spans/s, last {last:,.0f}, "
                  f"a fall of {(1 - last / first) * 100:.0f}%."),
        )

    # ------------------------------------------------------------ queries
    q = defaultdict(list)
    for r in rows:
        if r.get("phase") == "query":
            q[r["label"]].append(r)
    if q:
        rowcount = max((r.get("rows", 0) for r in rows if r.get("phase") == "query"), default=0)
        out = []
        order = ["list_traces", "get_trace", "search", "analytics_summary"]
        for label in sorted(q, key=lambda l: order.index(l) if l in order else 99):
            rs = q[label]
            lo, p50, hi, ratio = spread([x["p50_ms"] for x in rs])
            _, p99, _, _ = spread([x["p99_ms"] for x in rs])
            _, mx, _, _ = spread([x["max_ms"] for x in rs])
            out.append([
                label, len(rs), sum(x["repeats"] for x in rs),
                fnum(p50, 2), f"{fnum(lo, 2)} .. {fnum(hi, 2)}",
                fnum(p99, 2), fnum(mx, 2), sum(x["errors"] for x in rs),
            ])
        table(
            f"Read latency against a store of {rowcount:,} spans",
            ["endpoint", "reps", "requests", "p50 ms", "p50 range", "p99 ms", "max ms", "errors"],
            out,
            note="One connection, requests issued one after another.",
        )

    # ------------------------------------------- memory, store size, loaded
    seed = [r for r in rows if r.get("phase") == "seed"]
    if seed:
        s = seed[-1]
        spans = s["spans"]
        print()
        print("Footprint")
        print("---------")
        print(f"spans stored                {spans:,}")
        print(f"seed wall time              {s['seconds']:,.1f} s "
              f"({s['spans_per_sec']:,.0f} spans/s average)")
        print(f"daemon RSS at rest          {s['rss_at_rest_kb'] / 1024:,.1f} MiB")
        print(f"daemon RSS after the seed   {s['rss_loaded_kb'] / 1024:,.1f} MiB")
        print(f"store on disk               {s['store_kb'] / 1024:,.1f} MiB "
              f"(database file {s['db_file_kb'] / 1024:,.1f} MiB, rest is the WAL)")
        if spans:
            print(f"per span                    {s['store_kb'] * 1024 / spans:,.0f} bytes")

    sel = [r for r in rows if r.get("phase") == "search_selectivity"]
    if sel:
        table(
            "Search: the same endpoint, different terms",
            ["query", "p50 ms", "p99 ms", "traces returned"],
            [[r["label"], fnum(r["p50_ms"], 2), fnum(r["p99_ms"], 2), r["traces_returned"]]
             for r in sorted(sel, key=lambda r: -r["p50_ms"])],
            note="All limited to 50 traces. Cost tracks how many spans match, not how many are returned.",
        )

    loaded = [r for r in rows if r.get("phase") == "ingest_loaded"]
    if loaded:
        lo, mid, hi, ratio = spread([x["spans_per_sec"] for x in loaded])
        _, p50, _, _ = spread([x["p50_ms"] for x in loaded])
        _, p99, _, _ = spread([x["p99_ms"] for x in loaded])
        rowcount = loaded[0].get("rows", 0)
        print()
        print(f"Ingest into that {rowcount:,}-span store")
        print("-" * (len(f"Ingest into that {rowcount:,}-span store")))
        print(f"spans/s                     {fnum(mid)}  "
              f"(range {fnum(lo)} .. {fnum(hi)}, {ratio:.2f}x, {len(loaded)} reps)")
        print(f"p50 / p99 request           {fnum(p50, 1)} ms / {fnum(p99, 1)} ms "
              f"(batch {loaded[0]['batch']}, {loaded[0]['conns']} connection)")
        for r in rows:
            if r.get("phase") == "ingest_loaded_conns8":
                print(f"same store, {r['conns']} connections   {fnum(r['spans_per_sec'])} spans/s, "
                      f"p50 {fnum(r['p50_ms'] / 1000, 1)} s per request")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "results/results.jsonl")
