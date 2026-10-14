#!/usr/bin/env node
/**
 * Cross-language conformance.
 *
 * Every SDK is unit-tested on its own, which proves each does what its authors
 * meant and proves nothing about the property that matters: that all seven emit
 * the SAME trace for the same agent. `SDK-CONTRACT.md` calls behaviours B1 to
 * B10 binding in every language, and this is the only thing in the repository
 * that can check that sentence rather than restate it.
 *
 * Each probe writes one canonical run through its own SDK against one live
 * daemon. The harness reads all of them back through the HTTP API and asserts
 * they are structurally identical: the span tree, the five signals, the
 * correlation, the error, the token counts, the cost the daemon computed, and
 * the batch's idempotency key.
 *
 * A language whose toolchain is absent is REPORTED as skipped and counted, never
 * passed over quietly. A conformance suite that silently tests one language is
 * worse than none, because the badge still goes green.
 *
 * A language that DISAGREES is reported too. Differences already understood are
 * listed in `KNOWN_GAPS` with the file and function that causes them: they are
 * printed on every run and do not fail the suite, so the run still reports
 * instead of dying on the first one. A difference not in that list is a
 * failure, and so is an entry in the list for a difference nobody has any more.
 *
 * `mutations.mjs` corrupts a batch on its way to the daemon and asserts the
 * check that guards it goes red, so the checks below are things somebody has
 * watched fail rather than things somebody wrote.
 */

import { spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import http from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'
import zlib from 'node:zlib'

import { LANGUAGES, PROBES, ROOT, freePort, languageFilter, mcpdBinary } from './languages.mjs'

/** The run every probe writes. One copy, read by the harness and by a reader. */
const CANONICAL = JSON.parse(readFileSync(path.join(PROBES, 'canonical.json'), 'utf8'))
const PROJECT = CANONICAL.project

const bin = mcpdBinary()

if (!bin) {
  console.error('No mcpd binary. Run `cargo build -p mcpd` first.')
  process.exit(2)
}

const PORT = Number(process.env.MCPD_E2E_PORT) || (await freePort())
const ENDPOINT = `http://127.0.0.1:${PORT}`

/* ------------------------------------------------------------- assertions -- */

const ONLY = languageFilter()

const failures = []
const gaps = []
const skipped = []
let checks = 0
const perLanguage = {}

/**
 * Differences this suite has already found, named so the run still reports.
 *
 * Each key is `<language>:<the check that fails>` and each value says where the
 * behaviour is, so the entry is a work item rather than an excuse.
 *
 * Deleting an entry is how a fix is proved, and the audit at the end of this
 * file is what makes that true rather than a request: an entry whose check now
 * passes FAILS the run and names the line to delete. A list of accepted
 * differences is only safe while it is accurate, because an entry nobody needs
 * any more is exactly where the next real regression lands unreported. Ten of
 * the twelve entries this list held were already stale the first time it was
 * audited.
 */
const KNOWN_GAPS = {
}

/**
 * The same gaps, named by the field of the shape they show up in.
 *
 * A language listed here is left out of the cross-language comparison for that
 * ONE field, and compared on every other. The field is still compared between
 * every language not listed, so a new disagreement in a field an old one
 * already differs on is still a failure.
 *
 * Both lists are audited at the end of the run against what actually happened.
 * An entry for a behaviour somebody has since fixed fails the run and says
 * which line to delete, because a suppression with nothing under it is where
 * the next real regression would land unnoticed.
 */
const SHAPE_GAPS = {
  // Empty, and that is the point: every language now carries all five
  // correlation fields on the run and down to every step under it. The lists
  // are kept rather than deleted so a new divergence has an obvious place to be
  // recorded, and so the audit below keeps proving the suppression mechanism
  // itself still works.
  rootCorrelation: [],
  stepCorrelations: [],
}

/** Every `<language>:<check>` this run actually evaluated, and the subset of
 * those named in KNOWN_GAPS that still reproduced. The audit at the end reads
 * both: an entry naming a check nobody runs is a typo, and an entry whose check
 * passed is a gap somebody has closed and nobody has deleted. */
const evaluated = new Set()
const firedGaps = new Set()

function check(lang, what, condition, detail = '') {
  checks++
  perLanguage[lang] = (perLanguage[lang] ?? 0) + 1
  const key = `${lang}:${what}`
  evaluated.add(key)
  if (condition) return
  const line = `${lang}: ${what}${detail ? `, got ${detail}` : ''}`
  const gap = KNOWN_GAPS[key]
  if (gap) {
    firedGaps.add(key)
    gaps.push(`${line}\n      ${gap}`)
  } else {
    failures.push(line)
  }
}

/* --------------------------------------------------------------- reading -- */

async function api(pathname, query = {}) {
  const url = new URL(`${ENDPOINT}/v1/api${pathname}`)
  for (const [k, v] of Object.entries({ project: PROJECT, ...query })) {
    if (v !== undefined) url.searchParams.set(k, String(v))
  }
  const res = await fetch(url)
  if (!res.ok) throw new Error(`${url.pathname}${url.search} -> ${res.status} ${await res.text()}`)
  return res.json()
}

/** The correlation fields the daemon stores for each signal, and nothing more.
 *
 * A log keeps five of the twelve and a score keeps two; the rest are not
 * columns. Comparing a field the store does not keep would be a check that can
 * never fail, which is worse than no check at all. */
const STORED = {
  span: ['project', 'environment', 'service_name', 'service_version', 'session_id', 'thread_id',
    'user_id', 'resource_id', 'run_id', 'request_id', 'experiment_id', 'release'],
  log: ['project', 'environment', 'service_name', 'session_id', 'user_id'],
  score: ['project', 'environment'],
}

function pick(correlation, fields) {
  const out = {}
  for (const f of fields) if (correlation?.[f] !== undefined) out[f] = correlation[f]
  return out
}

/**
 * JSON with object keys in sorted order, at every level.
 *
 * `JSON.stringify(v, Object.keys(v).sort())` looks like this and is not: an
 * array replacer is an allowlist applied to nested objects too, so a nested key
 * absent from the top level's key list is silently dropped and two values that
 * differ only there compare equal.
 */
function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(',')}]`
  if (value === null || typeof value !== 'object') return JSON.stringify(value)
  const keys = Object.keys(value).sort()
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stable(value[k])}`).join(',')}}`
}

/** The shape every language must produce, derived from one trace. */
function shapeOf(detail, metric) {
  const spans = [...detail.spans].sort((a, b) => a.name.localeCompare(b.name))
  const root = detail.spans.find((s) => s.is_root)
  const gen = detail.spans.find((s) => s.usage)
  const failed = detail.spans.find((s) => s.status === 'error')
  const log = (detail.logs ?? [])[0]
  const scores = [...(detail.scores ?? [])].sort((a, b) => a.scorer_id.localeCompare(b.scorer_id))
  const feedback = (detail.feedback ?? [])[0]

  return {
    spanCount: detail.spans.length,
    rootName: root?.name,
    rootKind: root?.kind,
    traceStatus: detail.trace.status,
    kinds: spans.map((s) => `${s.name}:${s.kind}`),
    // Every child must hang off the root, not off nothing. A flat trace is the
    // classic SDK bug and reads as a broken agent rather than a broken SDK.
    //
    // The count is half the question, and it is the half that catches the bug.
    // mcpd derives `is_root` from the absence of a parent, so a flattened trace
    // comes back as four roots, `filter(s => !s.is_root)` is empty, and `every`
    // over an empty list is true. Asking only "does every child point at the
    // root" passed a completely flat trace, which is exactly the SDK bug this
    // line names.
    rootCount: detail.spans.filter((s) => s.is_root).length,
    parented:
      detail.spans.filter((s) => !s.is_root).length === detail.spans.length - 1
      && detail.spans.filter((s) => !s.is_root).every((s) => s.parent_span_id === root?.span_id),

    // Correlation, per signal, restricted to what the store keeps.
    rootCorrelation: pick(root?.correlation, STORED.span),
    stepCorrelations: spans
      .filter((s) => !s.is_root)
      .map((s) => stable(pick(s.correlation, STORED.span))),
    logCorrelation: pick(log?.correlation, STORED.log),
    scoreCorrelation: pick(scores[0]?.correlation, STORED.score),
    feedbackCorrelation: pick(feedback?.correlation, STORED.score),

    // The failure, recorded the same way in every language.
    erroredSpan: failed?.name,
    errorMessage: failed?.error?.message,
    errorStatus: failed?.status,
    otherSpansOk: spans.filter((s) => s.name !== failed?.name).every((s) => s.status === 'ok'),

    // What the model call cost, and what it was told it used.
    model: gen?.model,
    provider: gen?.provider,
    inputTokens: gen?.usage?.input_tokens,
    outputTokens: gen?.usage?.output_tokens,
    traceInputTokens: detail.trace.usage?.input_tokens,
    traceOutputTokens: detail.trace.usage?.output_tokens,
    spanCost: [gen?.cost?.currency, gen?.cost?.input_cost, gen?.cost?.output_cost,
      gen?.cost?.total_cost, gen?.cost?.price_book].join('|'),
    traceCost: [detail.trace.cost?.currency, detail.trace.cost?.total_cost].join('|'),

    // The other four signals.
    logCount: (detail.logs ?? []).length,
    logLevel: log?.level,
    logMessage: log?.message,
    logFields: stable(log?.fields),
    logOnTheRun: log?.trace_id === detail.trace.trace_id && log?.span_id === root?.span_id,
    scores: scores.map((s) => `${s.scorer_id}:${s.value}:${s.min}:${s.max}`),
    scoreSources: scores.map((s) => `${s.scorer_id}:${s.source}`),
    feedback: feedback ? `${feedback.kind}:${JSON.stringify(feedback.value)}` : undefined,
    metricValue: metric.point?.value,
    metricUnit: metric.unit,
    metricLabels: stable(metric.labels),
    metricExemplar: metric.point?.exemplar_trace_id === detail.trace.trace_id,
  }
}

/** One language's metric series, read back by the label the probe put on it. */
async function metricOf(lang, from, to) {
  const m = CANONICAL.metric
  const res = await api('/metrics', {
    name: m.name,
    from,
    to,
    agg: 'sum',
    group_by: 'outcome',
    filter: `lang:${lang}`,
  })
  const series = res.series?.[0]
  return {
    unit: res.unit,
    labels: series?.labels,
    point: series?.points?.[0],
    seriesCount: res.series?.length ?? 0,
  }
}

/* --------------------------------------------------------------- faults -- */

/**
 * One deliberate corruption of a batch, applied on the wire on its way in.
 *
 * Seven checks in this repository have reported success without testing
 * anything, so an assertion nobody has watched go red is not evidence. Each
 * entry here breaks exactly the thing one check guards, in the only place a
 * harness can break it for every language at once: the bytes the SDK sent.
 * `mutations.mjs` runs them all and asserts the named check went red.
 *
 * Editing a probe would prove the same thing for one language. This proves it
 * for all seven, and it survives, so the proof can be repeated after a change
 * rather than being a thing somebody once did by hand.
 */
const FAULTS = {
  // The classic SDK bug: every span emitted with no parent.
  'flat-trace': (b) => {
    for (const s of b.spans ?? []) {
      s.parent_span_id = null
      s.is_root = true
    }
  },
  // A step that failed, recorded as though it had not.
  'no-failure': (b) => {
    for (const s of b.spans ?? []) {
      if (s.status === 'error') {
        s.status = 'ok'
        delete s.error
      }
    }
  },
  // The failure recorded, on the wrong step. The run is still an error and
  // still has exactly one, so only the check that names the step can notice.
  'error-moves': (b) => {
    const spans = b.spans ?? []
    const failed = spans.find((s) => s.status === 'error')
    const other = spans.find((s) => s.status !== 'error' && !s.is_root && !s.usage)
    if (!failed || !other) return
    other.status = failed.status
    other.error = failed.error
    failed.status = 'ok'
    delete failed.error
  },
  'model-renamed': (b) => {
    for (const s of b.spans ?? []) if (s.model) s.model = 'gpt-4o-mini'
  },
  'usage-drift': (b) => {
    for (const s of b.spans ?? []) {
      if (s.usage?.input_tokens) s.usage.input_tokens += 1
    }
  },
  // A metric that no longer points at the run that produced it.
  'metric-unlinked': (b) => {
    for (const m of b.metrics ?? []) {
      m.trace_id = null
      m.span_id = null
      delete m.exemplar_trace_id
    }
  },
  'score-rescaled': (b) => {
    for (const s of b.scores ?? []) if (s.scorer_id === 'scale_check') s.max = 4
  },
  'score-devalued': (b) => {
    for (const s of b.scores ?? []) if (s.scorer_id === 'answer_relevance') s.value = 0.42
  },
  // Changes the shape and breaks no per-language check, so it is the fault that
  // puts the cross-language comparison itself to the test.
  'provider-renamed': (b) => {
    for (const s of b.spans ?? []) if (s.provider) s.provider = 'azure'
  },
}

const FAULT = process.env.MCPD_E2E_FAULT
if (FAULT && !FAULTS[FAULT]) {
  console.error(`MCPD_E2E_FAULT=${FAULT} names no fault. The suite knows: ${Object.keys(FAULTS).join(', ')}.`)
  process.exit(2)
}

/**
 * One language to corrupt, rather than all of them.
 *
 * A fault every language gets is a bug in every SDK at once, which the
 * per-language checks catch. A fault ONE language gets is the case this suite
 * exists for: six agree and the seventh does not. It is the only way to put the
 * cross-language comparison itself to the test.
 */
const FAULT_LANG = process.env.MCPD_E2E_FAULT_LANG
if (FAULT_LANG && !LANGUAGES.some((l) => l.id === FAULT_LANG)) {
  console.error(`MCPD_E2E_FAULT_LANG=${FAULT_LANG} names no probe.`)
  process.exit(2)
}

/**
 * Rewrite one batch, and the headers that describe it.
 *
 * The body is re-sent uncompressed because it is no longer the length or the
 * gzip stream the SDK's own headers describe, and a content-length that
 * disagrees with the body is a hang rather than a test.
 */
function corrupt(headers, body) {
  const encoding = headers['content-encoding'] ?? ''
  const raw = encoding.includes('gzip') ? zlib.gunzipSync(body) : body
  const batch = JSON.parse(raw.toString('utf8'))
  FAULTS[FAULT](batch)
  const out = Buffer.from(JSON.stringify(batch), 'utf8')
  const next = { ...headers }
  delete next['content-encoding']
  next['content-length'] = String(out.length)
  return { body: out, headers: next }
}

/* ----------------------------------------------------------- the recorder -- */

/**
 * A proxy in front of the daemon that keeps every batch a probe sent.
 *
 * Two things can only be checked from the wire. Whether an SDK put an
 * `ingest_key` on its batch at all, which `SDK-CONTRACT.md` section 6 requires
 * and no stored row records; and whether replaying that exact batch is refused
 * as a duplicate, which is the property the key exists for. Reading the stored
 * trace answers neither.
 */
const captured = []
let recordingFor = 'unknown'

const proxy = http.createServer((req, res) => {
  const chunks = []
  req.on('data', (c) => chunks.push(c))
  req.on('end', () => {
    const body = Buffer.concat(chunks)
    const ingest = req.method === 'POST' && req.url.startsWith('/v1/ingest')
    // Captured BEFORE any fault is applied, because what is captured is what
    // the SDK sent. The idempotency checks read these bytes back.
    if (ingest) captured.push({ lang: recordingFor, headers: { ...req.headers }, body })
    const corrupting = ingest && FAULT && (!FAULT_LANG || FAULT_LANG === recordingFor)
    const sent = corrupting ? corrupt(req.headers, body) : { body, headers: req.headers }
    const upstream = http.request(
      { host: '127.0.0.1', port: PORT, path: req.url, method: req.method, headers: sent.headers },
      (up) => {
        if (process.env.MCPD_E2E_DEBUG) console.error('[proxy]', req.method, req.url, '->', up.statusCode)
        res.writeHead(up.statusCode, up.headers)
        up.pipe(res)
      },
    )
    upstream.on('error', (err) => {
      res.writeHead(502, { 'content-type': 'text/plain' })
      res.end(String(err))
    })
    upstream.end(sent.body)
  })
})

function decode(record) {
  const encoding = record.headers['content-encoding'] ?? ''
  const raw = encoding.includes('gzip') ? zlib.gunzipSync(record.body) : record.body
  return JSON.parse(raw.toString('utf8'))
}

const SIGNALS = ['spans', 'logs', 'metrics', 'scores', 'feedback']
const sizeOf = (batch) => SIGNALS.reduce((n, k) => n + (batch[k]?.length ?? 0), 0)

/** Send one captured batch again, byte for byte, and report what mcpd did. */
async function replay(record) {
  const headers = { 'content-type': 'application/json' }
  if (record.headers['content-encoding']) {
    headers['content-encoding'] = record.headers['content-encoding']
  }
  const res = await fetch(`${ENDPOINT}/v1/ingest`, { method: 'POST', headers, body: record.body })
  if (!res.ok) throw new Error(`replay -> ${res.status} ${await res.text()}`)
  return res.json()
}

/* ------------------------------------------------------------------ main -- */

// Named from the pid AND a per-process nanosecond stamp. Pids are recycled, and
// a recycled one reopens the previous run's database, so every count is off by
// a whole prior suite.
const stamp = `${process.pid}-${process.hrtime.bigint()}`
const dataDir = mkdtempSync(path.join(tmpdir(), `mcpd-e2e-${stamp}-`))
const daemon = spawn(bin, ['serve', '--data', path.join(dataDir, 'e2e.db'), '--listen', `127.0.0.1:${PORT}`], {
  stdio: 'ignore',
})

let daemonExit = null
daemon.on('exit', (code, signal) => {
  daemonExit = signal ? `signal ${signal}` : `code ${code}`
})

let stopped = false
function stop() {
  if (stopped) return
  stopped = true
  daemon.kill()
  proxy.close()
  // The database is the whole of this suite's state. Left behind it is one
  // directory per run in the temp directory, and something for a stale daemon
  // to reopen.
  rmSync(dataDir, { recursive: true, force: true })
}

process.on('exit', stop)
// Node runs no `exit` handler for a default-handled signal, so an interrupted
// run would otherwise leave the daemon holding its port and its database.
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {
    stop()
    process.exit(130)
  })
}

const proxyPort = await freePort()
await new Promise((resolve) => proxy.listen(proxyPort, '127.0.0.1', resolve))
const PROBE_ENDPOINT = `http://127.0.0.1:${proxyPort}`

async function waitForDaemon() {
  for (let i = 0; i < 100; i++) {
    if (daemonExit) return false
    try {
      const r = await fetch(`${ENDPOINT}/healthz`)
      if (r.ok) return true
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 100))
  }
  return false
}

if (!(await waitForDaemon())) {
  console.error(
    daemonExit
      ? `The daemon exited with ${daemonExit} before it became healthy.`
      : `The daemon never became healthy on ${ENDPOINT}.`,
  )
  process.exit(2)
}

// The port was free when it was picked, so anything answering while our own
// daemon is dead is a different process, and every check below would be about
// somebody else's build against somebody else's data.
if (daemonExit) {
  console.error(`The daemon exited with ${daemonExit}, but ${ENDPOINT} still answers.`)
  console.error('Something else holds the port. Nothing below would be about this build.')
  process.exit(2)
}

const shapes = {}
const traceIds = {}

/**
 * The steps the canonical run names, and the kind each one has.
 *
 * Checked per language as well as across languages. The cross-language
 * comparison only says two languages differ, so a step that every SDK drops, or
 * a kind every SDK gets wrong, agrees perfectly and passes. This is the check
 * that would notice.
 */
const EXPECTED_STEPS = [CANONICAL.root, ...CANONICAL.children]
  .map((c) => ({ name: c.name, kind: c.kind }))
  .sort((a, b) => a.name.localeCompare(b.name))
  .map((c) => `${c.name}:${c.kind}`)

for (const lang of LANGUAGES) {
  if (ONLY.length > 0 && !ONLY.includes(lang.id)) {
    skipped.push(`${lang.id} (MCPD_E2E_LANGUAGES excludes it)`)
    continue
  }
  if (!lang.detect()) {
    skipped.push(`${lang.id} (${lang.why})`)
    continue
  }

  recordingFor = lang.id
  // The window this language's metric is read back over. It opens shortly before
  // its probe runs, so a point the SAME language wrote in an EARLIER run
  // against the same daemon is outside it and cannot be summed into this one.
  const probeStart = new Date(Date.now() - 5_000).toISOString()
  const env = {
    ...process.env,
    MCPD_ENDPOINT: PROBE_ENDPOINT,
    MCPD_PROJECT: PROJECT,
    MCPD_CONFORMANCE_LANG: lang.id,
    ...CANONICAL.environment,
  }
  const out = await lang.run(env)

  if (out.status !== 0) {
    failures.push(`${lang.id}: probe exited ${out.status}\n${(out.stderr || out.stdout || '').slice(0, 800)}`)
    continue
  }

  const traceId = (out.stdout || '').trim().split('\n').pop().trim()
  if (!/^[0-9a-f]{32}$/.test(traceId)) {
    failures.push(`${lang.id}: probe did not print a 32-hex trace id, got ${JSON.stringify(traceId.slice(0, 60))}`)
    continue
  }
  traceIds[lang.id] = traceId

  // A probe that printed a trace id the daemon does not have used to end the
  // whole run in an unhandled rejection, half way through the loop, with no
  // report for any language: the reads below are the first thing that touches
  // the daemon after the probe.
  let detail
  try {
    detail = await api(`/traces/${traceId}`, { include: 'logs,scores,feedback' })
  } catch (err) {
    failures.push(`${lang.id}: the probe printed trace ${traceId}, which the daemon does not have (${err.message})`)
    continue
  }
  const to = new Date(Date.now() + 60_000).toISOString()
  const metric = await metricOf(lang.id, probeStart, to)
  const shape = shapeOf(detail, metric)
  shapes[lang.id] = shape

  const id = lang.id
  const corr = CANONICAL.correlation
  const cost = CANONICAL.cost
  const log = CANONICAL.log
  const wanted = CANONICAL.children.find((c) => c.fails)

  // Per-language invariants, checked before the cross-language comparison so a
  // failure names what is wrong rather than only that two languages differ.
  check(id, 'the root span is the agent run', shape.rootKind === 'agent_run', shape.rootKind)
  check(id, 'the root span is the run the canonical run names',
    shape.rootName === CANONICAL.root.name, String(shape.rootName))
  // One check each, not two. `kinds` is compared as a list, so it is already
  // the count as well as the names, and `parented` counts the roots itself: the
  // pairs these replace could not fail apart, so counting both of each said the
  // suite checked more than it does.
  check(id, 'every step of the canonical run arrives, with its name and its kind',
    stable(shape.kinds) === stable(EXPECTED_STEPS),
    `${shape.spanCount} span(s): ${stable(shape.kinds)}`)
  check(id, 'the run has one root, and every other step hangs off it', shape.parented,
    `${shape.rootCount} root(s) of ${shape.spanCount} span(s)`)
  check(id, 'a failed step makes the whole run an error', shape.traceStatus === 'error', shape.traceStatus)
  check(id, 'only the failed step is an error', shape.otherSpansOk)
  check(id, 'ids are OpenTelemetry-shaped (B6)', detail.spans.every((s) => /^[0-9a-f]{16}$/.test(s.span_id)))

  // The error, which B10 says is a status AND a message, not a status alone.
  check(id, 'the failure lands on the step that failed', shape.erroredSpan === wanted.name, shape.erroredSpan)
  check(id, 'the error message survives the wire (B10)',
    shape.errorMessage === wanted.error_message, JSON.stringify(shape.errorMessage))

  // Correlation. The deployment three come from the environment variables the
  // contract fixes; the run's five are set on the span by the probe.
  const env3 = ['environment', 'service_name', 'service_version']
  check(id, 'the deployment environment reaches the wire',
    env3.every((f) => shape.rootCorrelation[f] === CANONICAL.environment[`MCPD_${f.toUpperCase()}`]),
    stable(pick(shape.rootCorrelation, env3)))
  check(id, 'the run carries every correlation field',
    shape.rootCorrelation.session_id === corr.session_id
    && shape.rootCorrelation.thread_id === corr.thread_id
    && shape.rootCorrelation.request_id === corr.request_id
    && shape.rootCorrelation.release === corr.release
    // The daemon hashes user_id at ingest, so the stored value is a digest of
    // it and never the value itself. Same input, same digest, in every language.
    && /^sha256:[0-9a-f]{24}$/.test(shape.rootCorrelation.user_id ?? ''),
    stable(shape.rootCorrelation))
  check(id, 'the run\'s correlation reaches every step',
    shape.stepCorrelations.every((c) => c === stable(shape.rootCorrelation)),
    shape.stepCorrelations[0])

  // Token counts and the cost mcpd computed from them. Two languages that
  // priced the same call differently is the difference a user notices first.
  const generation = CANONICAL.children.find((c) => c.model)
  check(id, 'the model call records its model', shape.model === 'gpt-4o', String(shape.model))
  // The provider was compared between languages and asserted nowhere, so seven
  // SDKs that all dropped it agreed perfectly and passed. It is half of what
  // prices a call: gpt-4o on Azure is not gpt-4o on OpenAI.
  check(id, 'the model call records its provider', shape.provider === generation.provider,
    String(shape.provider))
  check(id, 'token usage survives the wire', shape.inputTokens === 1180 && shape.outputTokens === 96,
    `${shape.inputTokens}/${shape.outputTokens}`)
  check(id, 'the run rolls the tokens up', shape.traceInputTokens === 1180 && shape.traceOutputTokens === 96,
    `${shape.traceInputTokens}/${shape.traceOutputTokens}`)
  check(id, 'mcpd prices the call the same way',
    shape.spanCost === [cost.currency, cost.input_cost, cost.output_cost, cost.total_cost, cost.price_book].join('|'),
    shape.spanCost)
  check(id, 'the run costs what its steps cost',
    shape.traceCost === [cost.currency, cost.total_cost].join('|'), shape.traceCost)

  // The signals the suite used to ignore.
  check(id, 'the log line arrives', shape.logCount === 1 && shape.logMessage === log.message,
    `${shape.logCount} log(s), ${JSON.stringify(shape.logMessage)}`)
  check(id, 'the log keeps its level and its fields',
    shape.logLevel === log.level && shape.logFields === stable(log.fields),
    `${shape.logLevel} ${shape.logFields}`)
  check(id, 'the log is attached to the run', shape.logOnTheRun)
  check(id, 'the log carries the run\'s correlation',
    shape.logCorrelation.session_id === corr.session_id
    && shape.logCorrelation.user_id === shape.rootCorrelation.user_id,
    stable(shape.logCorrelation))
  check(id, 'the metric point arrives', shape.metricValue === CANONICAL.metric.value, String(shape.metricValue))
  check(id, 'the metric keeps its unit and its labels',
    shape.metricUnit === CANONICAL.metric.unit
    && shape.metricLabels === stable(CANONICAL.metric.labels),
    `${shape.metricUnit} ${shape.metricLabels}`)
  check(id, 'the metric points back at the run that made it', shape.metricExemplar)
  // One series, not two. A second series under the same language label is a
  // point left over from an earlier run, and `agg: sum` would have folded it
  // into the value checked above.
  check(id, 'the metric series belongs to this run alone',
    metric.seriesCount === 1, `${metric.seriesCount} series`)

  const expected = CANONICAL.scores.map((s) => `${s.scorer_id}:${s.value}:${s.min}:${s.max}`)
  check(id, 'both scores land with their scale', stable(shape.scores) === stable(expected), stable(shape.scores))
  check(id, 'a score sent with a source keeps it',
    shape.scoreSources.includes('answer_relevance:live'), stable(shape.scoreSources))
  // Only where there is an SDK to have a default. `source` is required on the
  // wire, so `probe.sh` has to send one, and the check would be asserting what
  // the fixture says rather than what a language does.
  if (id !== 'curl') {
    check(id, 'a score sent with no source is live',
      shape.scoreSources.includes('scale_check:live'), stable(shape.scoreSources))
  }
  check(id, 'the feedback landed',
    shape.feedback === `${CANONICAL.feedback.kind}:${JSON.stringify(CANONICAL.feedback.value)}`,
    shape.feedback)

  // Filtering, which is how anyone actually finds a score or a piece of
  // feedback again. A signal that is stored but not findable is not stored.
  const byScorer = await api('/scores', { trace_id: traceId, scorer_id: 'answer_relevance' })
  check(id, 'a score is findable by its scorer', byScorer.scores.length === 1, String(byScorer.scores.length))
  const byValue = await api('/scores', { trace_id: traceId, min_value: 0.5, max_value: 1 })
  check(id, 'a score is findable by its value',
    byValue.scores.length === 1 && byValue.scores[0].scorer_id === 'answer_relevance',
    byValue.scores.map((s) => s.scorer_id).join(','))
  // By run, and the kind read off the record: `GET /v1/api/feedback` takes
  // the run and nothing else, and refuses a `kind` by name rather than
  // ignoring it, so the filter this check used to send is applied here.
  const byRun = await api('/feedback', { trace_id: traceId })
  const ofKind = byRun.feedback.filter((f) => f.kind === CANONICAL.feedback.kind)
  check(id, 'feedback is readable by its run and carries its kind', ofKind.length === 1,
    byRun.feedback.map((f) => f.kind).join(',') || 'none')

  // Idempotency, read off the wire the probe actually wrote.
  const sent = captured.filter((r) => r.lang === id).map((r) => ({ record: r, batch: decode(r) }))
  check(id, 'the SDK sent at least one batch', sent.length > 0, String(sent.length))
  if (sent.length > 0) {
    check(id, 'every batch carries an idempotency key (section 6)',
      sent.every(({ batch }) => typeof batch.ingest_key === 'string' && batch.ingest_key.length > 0),
      sent.map(({ batch }) => batch.ingest_key ?? 'none').join(' '))

    // Replayed even when the batch carries no key, because what happens then
    // is the thing the key exists to prevent.
    const biggest = sent.reduce((a, b) => (sizeOf(b.batch) > sizeOf(a.batch) ? b : a))
    const before = await api(`/traces/${traceId}`, { include: 'scores,feedback' })
    // A daemon that refuses the replay outright is a failure of this check, not
    // a reason to abandon every language after this one.
    let result
    try {
      result = await replay(biggest.record)
    } catch (err) {
      failures.push(`${lang.id}: replaying the batch was refused outright (${err.message})`)
      continue
    }
    const after = await api(`/traces/${traceId}`, { include: 'scores,feedback' })
    check(id, 'a replayed batch is refused as a duplicate',
      result.duplicate === sizeOf(biggest.batch) && result.accepted === 0,
      `accepted ${result.accepted}, duplicate ${result.duplicate}, batch of ${sizeOf(biggest.batch)}`)
    check(id, 'a replayed batch changes nothing',
      after.spans.length === before.spans.length
      && (after.scores ?? []).length === (before.scores ?? []).length
      && (after.feedback ?? []).length === (before.feedback ?? []).length,
      `${before.spans.length}->${after.spans.length} spans, `
      + `${(before.scores ?? []).length}->${(after.scores ?? []).length} scores`)
  }
}

/* The comparison this suite exists for. */
const ran = Object.keys(shapes)
const SHAPE_KEYS = ran.length > 0 ? Object.keys(shapes[ran[0]]) : []
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b)

/**
 * The language each field is compared against, chosen per field.
 *
 * Comparing everything against whichever language happened to run first left a
 * hole across the middle of this suite. A field listed in SHAPE_GAPS for that
 * first language was suppressed for EVERY pair, so a brand new disagreement
 * between two other languages in that field was reported as a known gap and
 * nothing failed. TypeScript runs first and was listed on two of these fields,
 * so those two were unguarded across all seven languages at once, which is
 * exactly the way a regression hides among the differences already accepted.
 *
 * The reference for a field is now the first language that ran and is NOT known
 * to deviate on it, so the order languages run in decides nothing.
 */
const reference = {}
for (const key of SHAPE_KEYS) {
  reference[key] = ran.find((l) => !(SHAPE_GAPS[key] ?? []).includes(l))
}

for (const other of ran) {
  const comparable = SHAPE_KEYS.filter(
    (k) => reference[k] !== undefined
      && reference[k] !== other
      && !(SHAPE_GAPS[k] ?? []).includes(other),
  )
  // One check per language, never one per field. A suite that counted a check
  // for every field it compared would report a number many times larger than
  // the number of things it can actually tell you.
  if (comparable.length === 0) continue
  checks++
  const differing = comparable.filter((k) => !same(shapes[reference[k]][k], shapes[other][k]))
  if (differing.length > 0) {
    failures.push(
      `${other} does not agree with the other languages:\n`
      + differing
        .map((k) => `      ${k}\n        ${reference[k]}: ${JSON.stringify(shapes[reference[k]][k])}`
          + `\n        ${other}: ${JSON.stringify(shapes[other][k])}`)
        .join('\n'),
    )
  }
}

/* ------------------------------------------------- the lists, kept honest -- */

/**
 * Both lists of accepted differences, audited against what actually happened.
 *
 * A list of known differences is only safe while it is true. An entry for a
 * behaviour somebody has since fixed is a suppression with nothing under it,
 * and the next real disagreement in that field lands under it and is reported
 * as already known. So a stale entry fails the run, and the failure says which
 * line to delete. That is also how a fix is proved: deleting the entry is the
 * commit that turns the check back into a failure if the behaviour returns.
 */
for (const [key, langs] of Object.entries(SHAPE_GAPS)) {
  if (ran.length > 0 && !SHAPE_KEYS.includes(key)) {
    failures.push(
      `SHAPE_GAPS names the field "${key}", which the compared shape no longer has. `
      + 'Rename it to the field it means, or delete it: an entry that matches no field '
      + 'suppresses nothing and reads as coverage that is not there.',
    )
    continue
  }
  const ref = reference[key]
  if (ref === undefined) continue
  for (const lang of langs) {
    if (!ran.includes(lang) || lang === ref) continue
    if (same(shapes[lang][key], shapes[ref][key])) {
      failures.push(
        `SHAPE_GAPS still lists ${lang} under ${key}, and ${lang} now agrees with ${ref} there. `
        + 'Delete it from SHAPE_GAPS in tests/e2e/run.mjs.',
      )
    }
  }
}

for (const key of Object.keys(KNOWN_GAPS)) {
  const lang = key.slice(0, key.indexOf(':'))
  // A language whose probe never got as far as its checks says nothing about
  // its gaps, and a language that did not run says nothing at all.
  if (!(perLanguage[lang] > 0)) continue
  if (!evaluated.has(key)) {
    failures.push(
      `KNOWN_GAPS names "${key}" and ${lang} ran no check by that name. Rename the entry to `
      + 'match the check, or delete it: an entry that matches no check suppresses nothing.',
    )
  } else if (!firedGaps.has(key)) {
    failures.push(
      `KNOWN_GAPS still names "${key}", and that check passed. Delete the entry from `
      + 'tests/e2e/run.mjs. Deleting it is how the fix is proved: the check is a failure '
      + 'again the moment the behaviour comes back.',
    )
  }
}

/* ------------------------------------------------------------- erasure -- */

/**
 * One erasure request, for the person every language said the run was for.
 *
 * This is a cross-language claim and not a daemon one. Each SDK sent the same
 * `user_id`, mcpd hashed each of them at ingest, and one deletion by subject
 * reaches all of them only if every language produced the same digest from the
 * same input. A language whose runs survive here is a language whose users
 * cannot be erased.
 *
 * It runs last, and only when nothing before it failed, because it destroys the
 * traces every earlier check compared.
 */
let erasureRan = false
if (ran.length > 0 && failures.length === 0) {
  erasureRan = true
  const subject = CANONICAL.correlation.user_id
  // What the audit log already held. A daemon this suite did not start (see
  // MCPD_E2E_PORT) has one entry per earlier run, and asserting a total of one
  // made the second run against the same daemon fail for a reason that has
  // nothing to do with any SDK.
  const auditBefore = (await api('/audit', { action: 'erase_subject', limit: 500 })).length
  const url = new URL(`${ENDPOINT}/v1/api/subjects/${encodeURIComponent(subject)}`)
  url.searchParams.set('project', PROJECT)
  const res = await fetch(url, { method: 'DELETE' })
  const erased = res.ok ? await res.json() : { error: `${res.status} ${await res.text()}` }

  check('erasure', 'one subject erases every language\'s run',
    erased.traces === ran.length,
    `${erased.traces} trace(s) erased where ${ran.length} language(s) ran: `
    + `${JSON.stringify(erased)}`)
  // Counted from the canonical run, never from a literal. A run that grows a
  // step or a score would otherwise need this number changed by hand, and the
  // hand that forgets leaves a check that passes for the wrong reason.
  check('erasure', 'erasing a run erases everything under it',
    erased.spans === EXPECTED_STEPS.length * ran.length
    && erased.logs === ran.length
    && erased.scores === CANONICAL.scores.length * ran.length
    && erased.feedback === ran.length,
    `${JSON.stringify(erased)}, wanted ${EXPECTED_STEPS.length * ran.length} spans, `
    + `${ran.length} logs, ${CANONICAL.scores.length * ran.length} scores, ${ran.length} feedback`)
  check('erasure', 'the total is the sum of the signals',
    erased.deleted === erased.traces + erased.spans + erased.logs + erased.metrics
      + erased.scores + erased.feedback,
    JSON.stringify(erased))

  const survivors = []
  for (const [lang, traceId] of Object.entries(traceIds)) {
    const after = await fetch(`${ENDPOINT}/v1/api/traces/${traceId}?project=${PROJECT}`)
    if (after.status !== 404) survivors.push(`${lang} (${after.status})`)
  }
  check('erasure', 'no language\'s run survives it', survivors.length === 0, survivors.join(', '))

  const audit = await api('/audit', { action: 'erase_subject', limit: 500 })
  check('erasure', 'the erasure is in the audit log',
    audit.length === auditBefore + 1 && audit[0]?.result === 'allowed',
    `${auditBefore} entr(ies) before, ${audit.length} after, newest `
    + `${JSON.stringify(audit[0] ?? null).slice(0, 160)}`)
  check('erasure', 'the audit log names the subject by its digest, never by name',
    audit.every((e) => e.subject !== subject && (e.resource ?? '') !== subject),
    JSON.stringify(audit.map((e) => [e.subject, e.resource])))
}

/* ---------------------------------------------------------------- report -- */

if (FAULT) {
  console.log(`\nFAULT INJECTED: ${FAULT}${FAULT_LANG ? ` into ${FAULT_LANG} alone` : ''}. `
    + 'Batches were corrupted on the way in, so this run')
  console.log('is a mutation test of the suite itself and cannot pass.')
}
console.log(`\nconformance: ${ran.length} language(s) ran, ${checks} checks`)
if (ran.length > 0 && !erasureRan) {
  console.log('  skip  erasure (it destroys the traces, so it only runs when nothing else failed)')
}
for (const l of ran) console.log(`  ok    ${l.padEnd(11)} ${perLanguage[l]} checks, trace ${traceIds[l]}`)
for (const s of skipped) console.log(`  skip  ${s}`)

if (gaps.length) {
  console.log(`\n${gaps.length} known difference(s) between languages:\n`)
  for (const g of gaps) console.log(`  !  ${g}`)
}

if (skipped.length && process.env.MCPD_REQUIRE_ALL_LANGUAGES === '1') {
  failures.push(`MCPD_REQUIRE_ALL_LANGUAGES is set and these were skipped: ${skipped.join(', ')}`)
}

if (ran.length < 2) {
  failures.push(`only ${ran.length} language ran; this suite compares languages, so it proves nothing alone`)
}

// A fault run that reports nothing is the one result that must never be quiet:
// the corruption reached the daemon and not one check here noticed it.
if (FAULT && failures.length === 0) {
  failures.push(
    `MCPD_E2E_FAULT=${FAULT} corrupted every batch on the way in and no check failed. `
    + 'Either the fault changes nothing, or nothing in this suite guards what it broke.',
  )
}

if (failures.length) {
  console.error(`\n${failures.length} failure(s):\n`)
  for (const f of failures) console.error(`  x ${f}`)
  stop()
  process.exit(1)
}

console.log('\nevery language produced the same trace.')
stop()
