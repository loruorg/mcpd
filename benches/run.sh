#!/usr/bin/env bash
#
# Measure a real mcpd daemon over HTTP.
#
# Every measurement starts its own daemon on an OS-assigned port with its own
# database, so no run inherits another run's rows. That matters more here than
# it usually does: per-span cost rises with the number of rows already stored,
# so a shared store would make the order of the runs part of the result.
#
#   ./run.sh            everything
#   QUICK=1 ./run.sh    the same shape, fewer repeats, for a smoke test
#
# Results land in benches/results/ as JSON lines plus a printed summary.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="${OUT:-$HERE/results}"
MCPD="${MCPD:-$ROOT/target/release/mcpd}"
BENCH="${BENCH:-$OUT/bench}"

# QUICK moves the defaults. An explicit setting still wins over it, so
# `QUICK=1 SEED_TOTAL=2000 ./run.sh` means what it looks like it means.
if [ "${QUICK:-0}" = "1" ]; then
  d_reps=1; d_spans=2000; d_seed=10000; d_qr=20
else
  d_reps=3; d_spans=5000; d_seed=100000; d_qr=50
fi
REPS="${REPS:-$d_reps}"
SPANS="${SPANS:-$d_spans}"
SEED_TOTAL="${SEED_TOTAL:-$d_seed}"
QUERY_REPEATS="${QUERY_REPEATS:-$d_qr}"
CONNS="${CONNS:-8}"
# Which phases to run. The loaded-store phase takes about twenty minutes to
# build its store, so being able to run the other two on their own is the
# difference between running this and not bothering.
PHASES="${PHASES:-sweep,decay,loaded}"
# Point this at an mcpd.db a previous run left behind to measure reads against
# it without seeding again. No daemon may be holding the file.
REUSE_DB="${REUSE_DB:-}"

wants() { case ",$PHASES," in *,"$1",*) return 0 ;; *) return 1 ;; esac; }

mkdir -p "$OUT"
RESULTS="$OUT/results.jsonl"
[ "${APPEND:-0}" = "1" ] || : > "$RESULTS"

log() { printf '%s\n' "$*" >&2; }

# An OS-assigned port, never a fixed one. A hardcoded port on a shared machine
# can reach somebody else's process and report a confident, wrong failure.
free_port() {
  python3 -c "import socket;s=socket.socket();s.bind(('127.0.0.1',0));print(s.getsockname()[1]);s.close()"
}

# The pid alone is recycled, and a recycled name reopens a previous run's
# database. The nanosecond stamp is what makes the name unique.
scratch() {
  python3 -c "import os,time,tempfile;print(os.path.join(tempfile.gettempdir(), f'mcpd-bench-{os.getpid()}-{time.time_ns()}'))"
}

sleep_s() { python3 -c "import sys,time;time.sleep(float(sys.argv[1]))" "$1"; }

DAEMON_PID=""
DAEMON_DIR=""
DAEMON_PORT=""

start_daemon() {  # $@ = extra daemon flags; START_FROM=path copies a store in first
  DAEMON_DIR="$(scratch)"
  mkdir -p "$DAEMON_DIR"
  if [ -n "${START_FROM:-}" ]; then
    cp "$START_FROM" "$DAEMON_DIR/mcpd.db" || return 1
  fi
  DAEMON_PORT="$(free_port)"
  MCPD_LOG=warn "$MCPD" serve \
    --listen "127.0.0.1:$DAEMON_PORT" \
    --data "$DAEMON_DIR/mcpd.db" "$@" > "$DAEMON_DIR/daemon.log" 2>&1 &
  DAEMON_PID=$!
  for _ in $(seq 1 200); do
    if curl -sf "http://127.0.0.1:$DAEMON_PORT/healthz" >/dev/null 2>&1; then return 0; fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
      log "daemon died on start:"; cat "$DAEMON_DIR/daemon.log" >&2; return 1
    fi
    sleep_s 0.05
  done
  log "daemon never became healthy on port $DAEMON_PORT"
  return 1
}

stop_daemon() {
  [ -n "$DAEMON_PID" ] || return 0
  kill "$DAEMON_PID" 2>/dev/null
  wait "$DAEMON_PID" 2>/dev/null
  DAEMON_PID=""
}

cleanup() {
  stop_daemon
  [ -n "$DAEMON_DIR" ] && rm -rf "$DAEMON_DIR"
}
trap cleanup EXIT

rss_kb() { awk '/VmRSS/ {print $2}' "/proc/$1/status" 2>/dev/null || echo 0; }

# Add fields to a result line so the summary can group without guessing.
tag() { python3 -c '
import json,sys
d = json.loads(sys.stdin.read() or "{}")
for kv in sys.argv[1:]:
    k, _, v = kv.partition("=")
    try: v = json.loads(v)
    except Exception: pass
    d[k] = v
print(json.dumps(d))' "$@"; }


# ------------------------------------------------------------------ preflight

if [ ! -x "$MCPD" ]; then
  log "building the daemon in release (a debug build measures the compiler, not mcpd)"
  ( cd "$ROOT" && cargo build --release -p mcpd ) || exit 1
fi
log "compiling the harness"
rustc -O -o "$BENCH" "$HERE/bench.rs" || exit 1

{
  echo "host:    $(uname -sr)"
  echo "cpu:     $(nproc) logical cores, $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ *//')"
  echo "memory:  $(free -g | awk '/^Mem:/ {print $2" GiB total, "$7" GiB available"}')"
  echo "disk:    $(df -T "$(dirname "$(scratch)")" | awk 'NR==2 {print $2" on "$1}')"
  echo "daemon:  $("$MCPD" --version 2>/dev/null | head -1)"
  echo "date:    $(date -Is)"
} | tee "$OUT/machine.txt" >&2
log ""

# -------------------------------------------------------------- ingest sweep
#
# Fixed work, not fixed time. Each run writes the same number of spans into an
# empty store, so every batch size is measured over the same stretch of the
# store-size curve. A fixed-duration run would measure each batch size at a
# different number of rows and the comparison would mean nothing.

if wants sweep; then
log "== ingest sweep: $SPANS spans into an empty store, $REPS repeats each =="
for redact in on off; do
  flags=()
  [ "$redact" = "off" ] && flags=(--no-redact)
  for batch in 1 10 100 1000; do
    for conns in 1 "$CONNS"; do
      for rep in $(seq 1 "$REPS"); do
        start_daemon "${flags[@]}" || exit 1
        line=$("$BENCH" ingest --url "127.0.0.1:$DAEMON_PORT" \
                 --batch "$batch" --conns "$conns" --spans "$SPANS" 2>>"$OUT/bench.err")
        rss=$(rss_kb "$DAEMON_PID")
        stop_daemon
        db=$(du -kc "$DAEMON_DIR"/mcpd.db* 2>/dev/null | tail -1 | cut -f1)
        rm -rf "$DAEMON_DIR"
        printf '%s' "$line" | tag "phase=sweep" "redact=\"$redact\"" "rep=$rep" \
          "rss_kb=${rss:-0}" "store_kb=${db:-0}" >> "$RESULTS"
        log "  redact=$redact batch=$batch conns=$conns rep=$rep -> $line"
      done
    done
  done
done
log ""
fi

# --------------------------------------------------------------- decay curve
#
# The same daemon, measured again and again as its store fills. This is the
# number that decides whether a single throughput figure means anything.

if wants decay; then
log "== decay: repeated windows against one growing store =="
start_daemon || exit 1
cum=0
for w in $(seq 1 12); do
  line=$("$BENCH" ingest --url "127.0.0.1:$DAEMON_PORT" \
           --batch 10 --conns 1 --seconds 2 --warmup 0 2>>"$OUT/bench.err")
  got=$(printf '%s' "$line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["spans"])')
  before=$cum
  cum=$((cum + got))
  printf '%s' "$line" | tag "phase=decay" "window=$w" "rows_before=$before" "rows_after=$cum" \
    >> "$RESULTS"
  log "  window $w  rows ${before}->${cum}  $line"
done
stop_daemon; rm -rf "$DAEMON_DIR"
log ""
fi

# ------------------------------------------------- the loaded store: queries
#
# Seeded through the daemon over HTTP, the same door an agent uses, so the rows
# are exactly what mcpd writes: the trace rollup, the search index and the
# denormalised columns all populated by the code that owns them. Building the
# file with direct SQL would be faster and would measure a store mcpd never
# actually produces.

if wants loaded; then
log "== loaded store: seed $SEED_TOTAL spans, then read =="
START_FROM="$REUSE_DB" start_daemon || exit 1
rest_kb=$(rss_kb "$DAEMON_PID")
log "  rss at rest: ${rest_kb} kB"
if [ -n "$REUSE_DB" ]; then
  # Reusing a store measures reads without paying for the seed again. The
  # RSS-after-seeding figure belongs to the run that did the seeding, so it is
  # not reported here: this daemon has ingested nothing.
  # Count the rows, not the counter. The accepted counter belongs to this
  # process, which has ingested nothing, so it would report an empty store.
  reused_rows=$(python3 -c "
import sqlite3,sys
c = sqlite3.connect('file:' + sys.argv[1] + '?mode=ro', uri=True)
print(c.execute('select count(*) from spans').fetchone()[0])" "$DAEMON_DIR/mcpd.db")
  seed_line=$(printf '{"mode":"seed","spans":%s,"seconds":0,"spans_per_sec":0,"errors":0,"reused":true}' \
                "${reused_rows:-0}")
  log "  reusing $REUSE_DB (no seeding)"
else
  seed_line=$("$BENCH" seed --url "127.0.0.1:$DAEMON_PORT" \
               --total "$SEED_TOTAL" --batch 100 --conns 4 2>>"$OUT/bench.err")
  log "  seed: $seed_line"
fi
loaded_kb=$(rss_kb "$DAEMON_PID")
store_kb=$(du -kc "$DAEMON_DIR"/mcpd.db* 2>/dev/null | tail -1 | cut -f1)
db_only_kb=$(du -k "$DAEMON_DIR/mcpd.db" | cut -f1)
printf '%s' "$seed_line" | tag "phase=seed" "rss_at_rest_kb=${rest_kb:-0}" \
  "rss_loaded_kb=${loaded_kb:-0}" "store_kb=${store_kb:-0}" "db_file_kb=${db_only_kb:-0}" \
  >> "$RESULTS"
log "  rss loaded: ${loaded_kb} kB   store: ${store_kb} kB (db file ${db_only_kb} kB)"

rows_now_q=$(printf '%s' "$seed_line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["spans"])')
for rep in $(seq 1 "$REPS"); do
  "$BENCH" query --url "127.0.0.1:$DAEMON_PORT" --repeats "$QUERY_REPEATS" 2>>"$OUT/bench.err" \
    | while read -r line; do
        printf '%s' "$line" | tag "phase=query" "rep=$rep" "rows=${rows_now_q:-$SEED_TOTAL}" >> "$RESULTS"
        log "  query rep=$rep $line"
      done
done

# What ingest costs once the store is full, which is the number a deployment
# that has been up for a week actually lives with.
#
# One connection, and a long window. A request here can take seconds, so a short
# window would finish with too few samples to call a median, and the extra
# connections only queue behind the single write lock.
rows_now=$(printf '%s' "$seed_line" | python3 -c 'import json,sys;print(json.load(sys.stdin)["spans"])')
for rep in $(seq 1 "$REPS"); do
  line=$("$BENCH" ingest --url "127.0.0.1:$DAEMON_PORT" \
           --batch 100 --conns 1 --seconds 20 --warmup 2 2>>"$OUT/bench.err")
  printf '%s' "$line" | tag "phase=ingest_loaded" "rep=$rep" "rows=${rows_now:-0}" >> "$RESULTS"
  log "  ingest into loaded store rep=$rep: $line"
done

# The same store with every connection pushing, which answers what concurrency
# buys against one write lock. Once, because the answer does not vary.
line=$("$BENCH" ingest --url "127.0.0.1:$DAEMON_PORT" \
         --batch 100 --conns "$CONNS" --seconds 40 --warmup 0 2>>"$OUT/bench.err")
printf '%s' "$line" | tag "phase=ingest_loaded_conns8" "rep=1" "rows=${rows_now:-0}" >> "$RESULTS"
log "  ingest into loaded store, $CONNS connections: $line"
stop_daemon
# Keep the seeded store: it cost twenty minutes and REUSE_DB can start from it.
if [ -z "$REUSE_DB" ]; then
  mv "$DAEMON_DIR/mcpd.db" "$OUT/seeded-${SEED_TOTAL}.db" 2>/dev/null \
    && log "  kept the seeded store at $OUT/seeded-${SEED_TOTAL}.db"
fi
rm -rf "$DAEMON_DIR"
fi

log ""
log "results: $RESULTS"
python3 "$HERE/summarise.py" "$RESULTS" | tee "$OUT/summary.txt"
