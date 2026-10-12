#!/usr/bin/env node
/**
 * The judge, against a real model.
 *
 * `judge.rs` has unit tests, and every one of them is about parsing: that JSON
 * survives a fenced code block, that a brace inside a string does not end the
 * object. None of them calls a model, so none of them can tell you whether the
 * thing works. The question this suite exists for is the one a mock cannot
 * answer: given a run that answers its question and a run that does not, does a
 * real model actually separate them, and does mcpd record the result honestly?
 *
 * It asserts a GAP, never a number. A judge at temperature zero is repeatable
 * enough to compare with itself and not repeatable enough to pin: the same
 * model behind the same gateway will return 0.9 one week and 0.95 the next, and
 * a test that expects 0.9 then fails for a reason that has nothing to do with
 * mcpd. What must hold is the ordering, and the size of the gap between the two
 * runs is what says the model read them rather than guessed.
 *
 * It also counts the calls that reach the model. Two of the assertions here are
 * about mcpd declining, and a decline that still spent money is a bug you would
 * never see from the response body alone.
 *
 * ## Credentials
 *
 * `LLM_API_ENDPOINT` and `LLM_API_TOKEN` come from the environment, or from
 * `.env`, which is gitignored. The token is never written to disk. The daemon's
 * config file has to be a real file on disk, and a bearer token in it would sit
 * there after the run, so the harness runs a small proxy on loopback instead:
 * the config points the judge at the proxy, and the proxy attaches the
 * credential in memory on the way out. Every graded request still reaches the
 * real gateway and is answered by the real model.
 *
 *     node tests/e2e/judge.mjs
 *
 * This costs money. It is not part of `make check` for that reason. With no
 * credentials it reports a skip and exits zero, unless you set
 * `MCPD_REQUIRE_LLM=1`, which turns the skip into a failure so that a pipeline
 * meant to run it cannot go green by doing nothing.
 */

import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'

const ROOT = path.resolve(import.meta.dirname, '..', '..')
const PROJECT = 'judge-e2e'

/* ----------------------------------------------------------- credentials -- */

/**
 * Read `.env` into the environment, without overwriting anything already set.
 *
 * The real environment wins, so CI can supply the credential as a secret while
 * a laptop reads the same names out of the file.
 */
function loadDotEnv(file) {
  if (!existsSync(file)) return
  for (const line of readFileSync(file, 'utf8').split('\n')) {
    const text = line.trim()
    if (!text || text.startsWith('#')) continue
    const eq = text.indexOf('=')
    if (eq < 1) continue
    const key = text.slice(0, eq).trim()
    let value = text.slice(eq + 1).trim()
    const quoted = value.length > 1 && (value.startsWith('"') || value.startsWith("'"))
    if (quoted && value.at(-1) === value[0]) value = value.slice(1, -1)
    if (!(key in process.env)) process.env[key] = value
  }
}

loadDotEnv(path.join(ROOT, '.env'))

const UPSTREAM = (process.env.LLM_API_ENDPOINT || '').replace(/\/+$/, '')
const TOKEN = process.env.LLM_API_TOKEN || ''
const MODEL = process.env.MCPD_JUDGE_MODEL || 'gpt-4o-mini'

if (!UPSTREAM || !TOKEN) {
  const missing = [!UPSTREAM && 'LLM_API_ENDPOINT', !TOKEN && 'LLM_API_TOKEN'].filter(Boolean)
  const why = `${missing.join(' and ')} ${missing.length > 1 ? 'are' : 'is'} not set`
  if (process.env.MCPD_REQUIRE_LLM === '1') {
    console.error(`judge e2e: MCPD_REQUIRE_LLM=1 and ${why}.`)
    console.error('Set both in the environment or in .env, or unset MCPD_REQUIRE_LLM.')
    process.exit(1)
  }
  console.log(`judge e2e: skipped, ${why}.`)
  console.log('Set them in .env to run this against a real model. It costs money.')
  console.log('Set MCPD_REQUIRE_LLM=1 to make this skip a failure instead.')
  process.exit(0)
}

const bin = ['target/debug/mcpd', 'target/release/mcpd']
  .map((p) => path.join(ROOT, p))
  .find(existsSync)

if (!bin) {
  console.error('judge e2e: no mcpd binary. Run `cargo build -p mcpd` first.')
  process.exit(2)
}

/* -------------------------------------------------------------- the pair -- */

/**
 * The two criteria this deployment grades against.
 *
 * Two rather than one because the criteria map is configuration, and a map with
 * a single entry never proves the daemon dispatches by key. Both are written so
 * the answers below separate on both: the second run is on a different subject
 * altogether, so it is neither relevant nor correct.
 */
const CRITERIA = {
  answer_relevance: 'Does the answer address the question that was asked?',
  answer_correctness: 'Is the answer a correct answer to the question that was asked?',
}

const QUESTION = 'What is the capital of France, and which river runs through it?'

/** A run that plainly answers its question. */
const GOOD = 'The capital of France is Paris, and the river running through it is the Seine.'

/**
 * A run that plainly does not. It is fluent, confident prose about something
 * else, which is the case a keyword scorer gets wrong and a reader does not.
 */
const BAD = 'Sourdough needs a live starter, strong flour and a long cold proof in the fridge.'

/* ------------------------------------------------------- the credentials -- */

/**
 * Loopback proxy that attaches the bearer token on the way out.
 *
 * `JudgeConfig.api_key` is read from the config file and nowhere else, so
 * configuring the judge with the real endpoint would mean writing the token to
 * disk. Everything else about the request is untouched: the daemon builds the
 * body, the gateway answers it, and the model is real.
 */
let upstreamCalls = 0
let upstreamFailure = null

const proxy = http.createServer((req, res) => {
  const chunks = []
  req.on('data', (c) => chunks.push(c))
  req.on('end', async () => {
    upstreamCalls++
    try {
      const reply = await fetch(`${UPSTREAM}${req.url}`, {
        method: req.method,
        headers: { 'content-type': 'application/json', authorization: `Bearer ${TOKEN}` },
        body: Buffer.concat(chunks),
      })
      const body = Buffer.from(await reply.arrayBuffer())
      if (!reply.ok) upstreamFailure = `${reply.status}: ${body.toString('utf8').slice(0, 300)}`
      res.writeHead(reply.status, { 'content-type': 'application/json' })
      res.end(body)
    } catch (e) {
      // Never let the message carry the request: the token is in the headers.
      upstreamFailure = `the gateway could not be reached: ${e.message}`
      res.writeHead(502, { 'content-type': 'application/json' })
      res.end('{"error":"proxy could not reach the gateway"}')
    }
  })
})

await new Promise((resolve) => proxy.listen(0, '127.0.0.1', resolve))
const proxyUrl = `http://127.0.0.1:${proxy.address().port}`

/* ------------------------------------------------------------ the daemon -- */

/** An unused loopback port, so this can run beside `make conformance`. */
function freePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer()
    probe.on('error', reject)
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address()
      probe.close(() => resolve(port))
    })
  })
}

const port = Number(process.env.MCPD_JUDGE_E2E_PORT) || (await freePort())
const ENDPOINT = `http://127.0.0.1:${port}`

// Named from the pid AND a per-process nanosecond stamp. Pids are recycled, and
// a recycled one reopens the previous run's database, so every count is off by
// a whole prior suite.
const stamp = `${process.pid}-${process.hrtime.bigint()}`
const dataDir = mkdtempSync(path.join(tmpdir(), `mcpd-judge-e2e-${stamp}-`))
const configFile = path.join(dataDir, 'mcpd.json')

// The api_key is left at its default of empty on purpose. It is the one field
// that would put a secret in this file, and the proxy supplies it instead.
writeFileSync(
  configFile,
  `${JSON.stringify({ judge: { endpoint: proxyUrl, model: MODEL, criteria: CRITERIA } }, null, 2)}\n`,
)

const daemon = spawn(
  bin,
  [
    'serve',
    '--data',
    path.join(dataDir, 'judge.db'),
    '--listen',
    `127.0.0.1:${port}`,
    '--config-file',
    configFile,
    '--project',
    PROJECT,
  ],
  { stdio: 'ignore' },
)

let exited = null
daemon.on('exit', (code, signal) => {
  exited = signal ? `signal ${signal}` : `code ${code}`
})

let stopped = false
function shutDown() {
  if (stopped) return
  stopped = true
  daemon.kill()
  proxy.close()
  // The config file names the judge's endpoint and model, and the database
  // holds the graded runs. Neither is wanted after the run, and one directory
  // per run accumulates in the temp directory.
  rmSync(dataDir, { recursive: true, force: true })
}

process.on('exit', shutDown)
// Node runs no `exit` handler for a default-handled signal, so an interrupted
// run would otherwise leave the daemon holding its port and its database. This
// suite spends money, so it is the one most likely to be interrupted.
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {
    shutDown()
    process.exit(130)
  })
}

async function waitForDaemon() {
  for (let i = 0; i < 100; i++) {
    if (exited) return false
    try {
      if ((await fetch(`${ENDPOINT}/healthz`)).ok) return true
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 100))
  }
  return false
}

if (!(await waitForDaemon())) {
  console.error(
    exited
      ? `judge e2e: the daemon exited with ${exited} before it became healthy.`
      : `judge e2e: the daemon never became healthy on ${ENDPOINT}.`,
  )
  shutDown()
  process.exit(2)
}

// The port was free when it was picked, so anything answering while our own
// daemon is dead is a different process. This suite would then grade runs on
// somebody else's daemon, through somebody else's judge configuration, and pay
// for the model calls.
if (exited) {
  console.error(`judge e2e: the daemon exited with ${exited}, but ${ENDPOINT} still answers.`)
  console.error('Something else holds the port. Nothing below would be about this build.')
  shutDown()
  process.exit(2)
}

/* ----------------------------------------------------------------- wire -- */

const hex = (n) => randomBytes(n).toString('hex')

async function post(pathname, body) {
  const res = await fetch(`${ENDPOINT}${pathname}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  return { status: res.status, body: await res.json().catch(() => ({})) }
}

async function get(pathname) {
  const res = await fetch(`${ENDPOINT}/v1/api${pathname}`)
  if (!res.ok) throw new Error(`${pathname} -> ${res.status} ${await res.text()}`)
  return res.json()
}

/**
 * Write one run through the HTTP API and return its trace id.
 *
 * `answer` of null is a run that recorded nothing to grade. It carries a tool
 * call so it is a real run that got part-way, not an empty trace, and no span
 * in it holds an output: `Target::output_text` falls back to the last model
 * generation, so an answer hidden there would not be nothing to grade.
 */
async function ingest({ question, answer }) {
  const traceId = hex(16)
  const rootId = hex(8)
  const childId = hex(8)
  const start = new Date(Date.now() - 1200).toISOString()
  const end = new Date().toISOString()
  const correlation = { project: PROJECT }
  const base = {
    trace_id: traceId,
    status: 'ok',
    start_time: start,
    end_time: end,
    duration_ns: 1_200_000_000,
    is_event: false,
    received_at: '1970-01-01T00:00:00.000Z',
    correlation,
  }

  const root = {
    ...base,
    span_id: rootId,
    parent_span_id: null,
    name: 'support_agent',
    kind: 'agent_run',
    is_root: true,
    input: question,
    ...(answer === null ? {} : { output: answer }),
  }

  const child = {
    ...base,
    span_id: childId,
    parent_span_id: rootId,
    name: 'lookup_reference',
    kind: 'tool_call',
    is_root: false,
  }

  const res = await post('/v1/ingest', { spans: [root, child] })
  if (res.status !== 200 && res.status !== 202) {
    throw new Error(`ingest -> ${res.status} ${JSON.stringify(res.body)}`)
  }
  return traceId
}

/* ----------------------------------------------------------- assertions -- */

const failures = []
let checks = 0

function check(what, condition, detail = '') {
  checks++
  if (condition) {
    console.log(`  ok    ${what}`)
    return true
  }
  console.log(`  FAIL  ${what}${detail ? `: ${detail}` : ''}`)
  failures.push(`${what}${detail ? `: ${detail}` : ''}`)
  return false
}

function finish() {
  console.log(`\njudge e2e: ${checks} checks, ${upstreamCalls} model call(s)`)
  if (failures.length) {
    console.error(`\n${failures.length} failure(s):\n`)
    for (const f of failures) console.error(`  ✗ ${f}`)
    shutDown()
    process.exit(1)
  }
  console.log(`the judge (${MODEL}) separated the two runs.`)
  shutDown()
  process.exit(0)
}

/* ------------------------------------------------------------------ run -- */

console.log(`judge e2e: model ${MODEL}, ${Object.keys(CRITERIA).length} criteria\n`)

const good = await ingest({ question: QUESTION, answer: GOOD })
const bad = await ingest({ question: QUESTION, answer: BAD })
const unanswered = await ingest({ question: QUESTION, answer: null })

/* 1. The pair, graded by the real model. */

const scorers = Object.keys(CRITERIA)
const graded = await post('/v1/api/score', { project: PROJECT, trace_ids: [good, bad], scorers })

if (!check('the scoring pass succeeded', graded.status === 200, `${graded.status} ${JSON.stringify(graded.body).slice(0, 300)}`)) {
  if (upstreamFailure) failures.push(`the gateway said: ${upstreamFailure}`)
  finish()
}

const produced = graded.body.scores || []
const skipped = graded.body.skipped || []

check(
  'every criterion graded every run',
  produced.length === scorers.length * 2,
  `got ${produced.length} score(s), skipped ${JSON.stringify(skipped)}`,
)

check(
  'grading the pair cost one model call per criterion per run',
  upstreamCalls === scorers.length * 2,
  `${upstreamCalls} call(s)`,
)

const valueOf = (traceId, scorerId) =>
  produced.find((s) => s.trace_id === traceId && s.scorer_id === scorerId)?.value

// The size that says the model read the runs rather than guessed. The observed
// gap is far wider, so this threshold is not a target the model has to hit: it
// is the floor below which the result would no longer mean anything. Pinning
// the real number instead would fail on the week the gateway changes a
// checkpoint, which is a fact about the model and not about mcpd.
const MIN_GAP = 0.3

for (const scorerId of scorers) {
  const hi = valueOf(good, scorerId)
  const lo = valueOf(bad, scorerId)
  const gap = hi - lo
  check(
    `${scorerId}: the answering run scores at least ${MIN_GAP} above the one that does not`,
    Number.isFinite(gap) && gap >= MIN_GAP,
    `answered ${hi}, did not answer ${lo}, gap ${Number.isFinite(gap) ? gap.toFixed(3) : 'none'}`,
  )
}

/* 2. What the store kept. */

const stored = await get(`/scores?project=${PROJECT}&trace_id=${good}&limit=50`)
const judged = (stored.scores || []).filter((s) => scorers.includes(s.scorer_id))

check('the judged scores were written, not only returned', judged.length === scorers.length, `${judged.length} stored`)

check(
  'every stored judged score records the model as its scorer_version',
  judged.length > 0 && judged.every((s) => s.scorer_version === MODEL),
  JSON.stringify(judged.map((s) => s.scorer_version)),
)

// `every` over an empty list is true, so each of these repeats the count. A run
// that stored nothing would otherwise pass both of them while the check above
// carried the whole failure on its own.
check(
  'every stored judged score carries the scale it was graded on',
  judged.length === scorers.length && judged.every((s) => s.min === 0 && s.max === 1),
  `${judged.length} stored, ${JSON.stringify(judged.map((s) => [s.min, s.max]))}`,
)

check(
  'the judge gave a reason for its number',
  judged.length === scorers.length
  && judged.every((s) => typeof s.reason === 'string' && s.reason.trim().length > 0),
  `${judged.length} stored, ${JSON.stringify(judged.map((s) => s.reason))}`,
)

/* 3. A run with nothing to grade declines, and does not spend money doing it. */

const before = upstreamCalls
const nothing = await post('/v1/api/score', {
  project: PROJECT,
  trace_ids: [unanswered],
  scorers: ['answer_relevance'],
})

check('scoring a run with no answer is not an error', nothing.status === 200, String(nothing.status))

const declines = nothing.body.skipped || []
check(
  'a run with no answer is declined, not scored',
  (nothing.body.scores || []).length === 0 && declines.length === 1,
  JSON.stringify(nothing.body).slice(0, 300),
)

check(
  'the decline names the run and the scorer it is about',
  declines[0]?.trace_id === unanswered && declines[0]?.scorer_id === 'answer_relevance',
  JSON.stringify(declines[0]),
)

check(
  'the decline carries a reason a reader can act on',
  /no answer to grade/.test(declines[0]?.reason || ''),
  JSON.stringify(declines[0]?.reason),
)

check(
  'declining a run with no answer calls no model',
  upstreamCalls === before,
  `${upstreamCalls - before} call(s) were made`,
)

/* 4. An unknown criterion. */

// A criterion that is not configured is refused at the door, before any run is
// read and before any model is called, and the refusal lists what you could
// have named instead. `Declined::UnknownCriterion` is the guard behind that,
// and it is the reason a typo costs nothing rather than grading every run in
// the project against a question nobody wrote.
const unknownBefore = upstreamCalls
const unknown = await post('/v1/api/score', {
  project: PROJECT,
  trace_ids: [good],
  scorers: ['answer_smell'],
})

check('an unknown criterion is refused', unknown.status === 400, String(unknown.status))

check(
  'the refusal names the criterion that does not exist',
  (unknown.body.message || '').includes('answer_smell'),
  JSON.stringify(unknown.body).slice(0, 300),
)

check(
  'the refusal lists the criteria that do exist',
  scorers.every((id) => (unknown.body.message || '').includes(id)),
  JSON.stringify(unknown.body.message || '').slice(0, 400),
)

check(
  'an unknown criterion calls no model',
  upstreamCalls === unknownBefore,
  `${upstreamCalls - unknownBefore} call(s) were made`,
)

finish()
