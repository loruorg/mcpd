# benches

A load generator and a stopwatch for a running mcpd daemon.

Nothing here calls into the crates. Every measurement goes over a socket to a
real `mcpd serve` process, because the number an operator sizes a deployment
with includes the whole path: accept, decode, sample, redact, price, quota,
transaction. A microbenchmark of `Store::ingest` would report a faster number
that nobody can plan with.

There is no benchmarking framework and no dependency. `bench.rs` is one file of
plain Rust that `rustc -O` compiles in about two seconds. A loop, a clock and a
sorted vector of samples answer the question.

## Run it

```sh
cd benches
./run.sh
```

The script builds the daemon in release if `target/release/mcpd` is missing,
compiles the harness, then runs three phases. It writes `results/results.jsonl`,
`results/machine.txt` and a printed summary in `results/summary.txt`.

A release build is not optional. A debug build measures rustc, not mcpd.

### Knobs

| Variable | Default | What it does |
| --- | --- | --- |
| `PHASES` | `sweep,decay,loaded` | Which phases to run. |
| `REPS` | `3` | Repeats per measurement. Never report one sample. |
| `SPANS` | `5000` | Spans written per sweep measurement. |
| `SEED_TOTAL` | `100000` | Spans in the store the read numbers are measured against. |
| `QUERY_REPEATS` | `50` | Requests per read endpoint per repeat. |
| `CONNS` | `8` | The concurrent arm of the sweep. |
| `REUSE_DB` | unset | Path to a store a previous run left behind. Skips seeding. |
| `QUICK` | `0` | Smaller everything, for checking the harness works. |
| `MCPD` | `../target/release/mcpd` | The binary under test. |

Seeding 100k spans takes about twenty minutes, so the loaded phase keeps its
store at `results/seeded-100000.db`. To measure reads again without paying for
that:

```sh
PHASES=loaded REUSE_DB=results/seeded-100000.db ./run.sh
```

### One measurement at a time

`bench.rs` is usable on its own against any daemon you already have running.

```sh
rustc -O -o bench bench.rs
./bench ingest --url 127.0.0.1:4317 --batch 100 --conns 8 --spans 5000
./bench ingest --url 127.0.0.1:4317 --batch 10 --conns 1 --seconds 5 --warmup 1
./bench seed   --url 127.0.0.1:4317 --total 100000 --batch 100 --conns 4
./bench query  --url 127.0.0.1:4317 --repeats 50
```

Each command prints one JSON object per line. That is the whole interface
between the harness and the summary.

## What each number means

### Ingest

**spans/s** is spans the daemon reported as `accepted`, divided by wall time.
Rejected and duplicate spans are counted separately and never added in, so a
daemon that refuses everything reports zero rather than a fast number.

**p50 ms and p99 ms** are per HTTP request, not per span. A batch of 1000 spans
is one sample. Compare latency down a column, not across the batch column: the
same 20 ms means something different for one span and for a thousand.

The mean is printed as well but the median and the 99th are the ones to read.
The mean hides the stall that wakes somebody up.

The sweep writes a **fixed number of spans into an empty store**, not for a
fixed time. This matters because per-span cost rises with the number of rows
already stored. A fixed-duration run would leave each batch size sitting at a
different point on that curve, and the batch sizes would not be comparable.

The load is **closed loop**: each connection sends the next batch as soon as the
last reply lands. So the latency is service time at that offered load. It is not
the queueing latency you would see from a fixed arrival rate that exceeds what
the daemon can absorb.

### The price of redaction

The same sweep with the daemon started `--no-redact`. The gain column is how
much faster ingest got with the privacy control switched off. Redaction is on by
default, so the `on` column is what a deployment that changed nothing is paying.

The payload the harness sends carries keys the default redactor matches
(`api_key`, `email`, `user_id`) inside a nested `attributes` object. Without
them the comparison would measure a walk over a document with nothing to redact,
which is not the question.

### Decay

One daemon, measured again and again as its own store fills. Read this table
before you quote any single throughput number. If the last window is much slower
than the first, throughput is a function of how much you have already stored,
and one figure without a row count beside it is not usable.

### Read latency

Four endpoints against a store of `SEED_TOTAL` spans:

- `list_traces`: `GET /v1/api/traces?limit=50`, the trace list.
- `get_trace`: `GET /v1/api/traces/{id}`, one trace and its spans. The id comes
  from the list, so this is a hit, not the not-found path.
- `search`: `GET /v1/api/traces?q=...`, full text over prompts and completions.
- `analytics_summary`: `GET /v1/api/analytics/summary`, the default 24 hour
  window. The seeded spans are stamped inside that window on purpose. Spans
  dated 1970 would leave the summary with nothing to aggregate and the number
  would be a measurement of an empty result.

One connection, requests one after another, because a read latency is what one
person waits for when they open a trace.

### Footprint

**RSS at rest** is the daemon just after it becomes healthy, before any signal.
**RSS after the seed** is the same process once it has written every span. Both
come from `/proc/<pid>/status`, so they are resident pages, not heap.

**Store on disk** is `mcpd.db` plus its WAL and shared-memory files, because all
three are on the disk you are paying for. The database file alone is reported
next to it. The WAL is checkpointed on a schedule, so the split moves; the total
is the honest figure for sizing a volume.

**Per span** is that total divided by spans stored. Multiply it by your span rate
to size retention. It is specific to this payload: the harness sends a realistic
span of roughly a kilobyte with a prompt, a completion, nested attributes and
usage. A deployment that stores longer prompts pays more per span.

## What these numbers do not say

- One machine, one filesystem. `results/machine.txt` records which. A different
  disk moves every write number.
- SQLite only. Postgres and ClickHouse are behind Cargo features and the harness
  has never been pointed at them.
- No TLS and no proxy in front. Loopback only, so no network is measured.
- The sweep starts from an empty store. Real deployments do not run there. The
  decay table and the loaded-store measurement are the ones that describe a
  daemon that has been up for a while.
- Nothing here measures the SDKs. This is the daemon.

## Files

| File | What it is |
| --- | --- |
| `bench.rs` | The harness. Zero dependencies, builds with `rustc -O`. |
| `run.sh` | Starts daemons, runs the phases, collects results. |
| `summarise.py` | Turns `results.jsonl` into the printed tables. |
| `results/` | Output. Not committed. |

## Notes for whoever runs this next

Every daemon gets a port the OS assigns and a directory named from the pid and a
nanosecond stamp. Neither is a style choice. A fixed port on a shared machine can
reach a process that is not mcpd and report a confident, wrong result, and a
directory named from the pid alone reopens an earlier run's database, because
pids are recycled.
