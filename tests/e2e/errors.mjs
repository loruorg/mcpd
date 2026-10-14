#!/usr/bin/env node
/**
 * The error paths a real deployment hits.
 *
 * Every SDK claims three things in `SDK-CONTRACT.md`: B1, never throw from
 * instrumentation; B2, never block the agent on the network; B5, `shutdown()`
 * flushes and returns. Each SDK's own unit tests check those against a fake
 * transport, which is the one place they cannot fail. This suite checks them
 * against a server that is genuinely broken.
 *
 * Seven ways a deployment breaks, each stood up for real:
 *
 *   refused    nothing is listening on the port at all
 *   no host    the endpoint does not resolve at all
 *   500        the daemon answers every batch with an error
 *   hangup     the daemon writes a header and drops the connection
 *   garbage    the daemon answers 200 with a body that is not JSON
 *   413        the batch is larger than the daemon will accept
 *   bad key    a real, keyed daemon refuses the batch as unauthorised
 *
 * For each, every probe runs the canonical agent and must still finish: exit
 * zero, print its trace id, and take no longer than it took against a healthy
 * daemon. The agent has to survive its telemetry.
 *
 * The `curl` probe is the control. It is raw HTTP with no SDK, so it MUST fail
 * every scenario. If it passes, the scenario was not hostile and every other
 * language passed it for nothing, which is the shape of a suite that tests
 * nothing at all.
 *
 * The second half is the daemon's side of the same question: an oversized body,
 * malformed JSON, a batch of invalid items and an unauthorised key. What is
 * being checked is not only the status code but that the daemon is still
 * serving afterwards, and still accepts a good batch. A hostile client must not
 * be able to take mcpd down.
 *
 *     node tests/e2e/errors.mjs
 *
 * `MCPD_E2E_LANGUAGES=typescript,python` narrows it while working on it.
 * Excluded languages are reported as skips, never passed over quietly.
 */

import { spawn } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { LANGUAGES, ROOT, freePort, languageFilter, mcpdBinary } from './languages.mjs'

const PROJECT = 'errors-e2e'
const INGEST_KEY = 'ingest-errors-e2e'

const bin = mcpdBinary()

if (!bin) {
  console.error('No mcpd binary. Run `cargo build -p mcpd` first.')
  process.exit(2)
}

const ONLY = languageFilter()

/* ------------------------------------------------------------- assertions -- */

const failures = []
const gaps = []
const skipped = []
let checks = 0

/**
 * Differences this suite has already found, named so the run still reports.
 *
 * Each key is `<language>:<the check that fails>` and each value says where the
 * behaviour is, so the entry is a work item rather than an excuse. Deleting an
 * entry is how a fix is proved: the check goes back to being a failure the
 * moment the gap is closed. This is the same arrangement `run.mjs` uses, for
 * the same reason: one open gap must not be able to hide the next one.
 */
const KNOWN_GAPS = {
  'elixir:the SDK keeps its diagnostics off the agent\'s stdout':
    'warn_once in sdks/elixir/lib/mcpd/worker.ex calls Logger.warning, and Elixir\'s default '
    + 'console backend writes to stdout rather than stderr. Every other SDK writes its '
    + 'diagnostic to stderr. The line can arrive after the program\'s own output, so a run '
    + 'whose result is read from stdout gets telemetry in the middle of it.',
}

/** Every `<language>:<check>` this run evaluated, and the ones named in
 * KNOWN_GAPS that still reproduced. Audited at the end: an entry naming a check
 * nobody runs is a typo, and an entry whose check passed everywhere is a gap
 * somebody has closed and nobody has deleted. A suppression with nothing under
 * it is where the next real regression lands unnoticed. */
const evaluated = new Set()
const firedGaps = new Set()

function check(where, lang, what, condition, detail = '') {
  checks++
  const key = `${lang}:${what}`
  evaluated.add(key)
  if (condition) return
  const line = `${where}: ${what}${detail ? `\n      ${detail}` : ''}`
  const gap = KNOWN_GAPS[key]
  if (gap) {
    firedGaps.add(key)
    gaps.push(`${line}\n      ${gap}`)
  } else {
    failures.push(line)
  }
}

/* ---------------------------------------------------------------- daemons -- */

// Named from the pid AND a per-process nanosecond stamp. Pids are recycled, and
// a recycled one reopens the previous run's database, so every count is off by
// a whole prior suite.
const stamp = `${process.pid}-${process.hrtime.bigint()}`
const dataDir = mkdtempSync(path.join(tmpdir(), `mcpd-errors-e2e-${stamp}-`))

const servers = []
const children = []
let stopped = false

function stop() {
  if (stopped) return
  stopped = true
  for (const c of children) c.kill()
  for (const s of servers) s.close()
  rmSync(dataDir, { recursive: true, force: true })
}

process.on('exit', stop)
// Node runs no `exit` handler for a default-handled signal, so an interrupted
// run would otherwise leave two daemons holding their ports and their databases.
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => {
    stop()
    process.exit(130)
  })
}

/** Start a daemon on a port the operating system says is free. */
async function daemon(name, extra = []) {
  const port = await freePort()
  const endpoint = `http://127.0.0.1:${port}`
  const child = spawn(
    bin,
    ['serve', '--data', path.join(dataDir, `${name}.db`), '--listen', `127.0.0.1:${port}`,
      '--project', PROJECT, ...extra],
    { stdio: 'ignore' },
  )
  children.push(child)

  let exited = null
  child.on('exit', (code, signal) => {
    exited = signal ? `signal ${signal}` : `code ${code}`
  })

  for (let i = 0; i < 100; i++) {
    if (exited) break
    try {
      if ((await fetch(`${endpoint}/healthz`)).ok) {
        // The port was free when it was picked, so anything answering while our
        // own daemon is dead is a different process, and every check below
        // would be about somebody else's build.
        if (exited) break
        return { endpoint, child }
      }
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 100))
  }
  console.error(
    exited
      ? `The ${name} daemon exited with ${exited} before it became healthy.`
      : `The ${name} daemon never became healthy on ${endpoint}.`,
  )
  stop()
  process.exit(2)
}

const keyedConfig = path.join(dataDir, 'keyed.json')
writeFileSync(
  keyedConfig,
  JSON.stringify({ projects: { [PROJECT]: { ingest_keys: [INGEST_KEY] } } }),
)

const open = await daemon('open')
const keyed = await daemon('keyed', ['--config-file', keyedConfig])

/* -------------------------------------------------------- hostile servers -- */

/**
 * A server that answers every batch badly, in one named way.
 *
 * It counts the requests it received. A scenario where the SDK never sent
 * anything would pass every assertion below without the hostile behaviour ever
 * being reached, so the count is asserted too.
 */
async function hostile(behaviour) {
  const state = { requests: 0 }
  const server = http.createServer((req, res) => {
    state.requests++
    const drain = []
    req.on('data', (c) => drain.push(c))
    req.on('end', () => {
      if (behaviour === 'status500') {
        res.writeHead(500, { 'content-type': 'application/json' })
        res.end('{"error":"internal","message":"the store is down"}')
      } else if (behaviour === 'garbage') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end('<html>a proxy replaced your response</html>')
      } else if (behaviour === 'toolarge') {
        res.writeHead(413, { 'content-type': 'application/json' })
        res.end('{"error":"payload_too_large","message":"batch is larger than 32 MiB"}')
      } else if (behaviour === 'hangup') {
        // A header, then the socket goes away with the body half written. This
        // is what a load balancer draining a connection looks like to a client.
        res.socket.write('HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 64\r\n\r\n{"acce')
        res.socket.destroy()
      } else if (behaviour === 'blackhole') {
        // Never answers, and never closes. The request is held open until the
        // suite tears the server down.
        state.held = state.held ?? []
        state.held.push(res)
      }
    })
  })
  servers.push(server)
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return { url: `http://127.0.0.1:${server.address().port}`, state, server }
}

/** A port bound and then released, so a connection to it is refused. */
const refusedPort = await freePort()

/* ------------------------------------------------------------- the probes -- */

const runnable = []
for (const lang of LANGUAGES) {
  if (ONLY.length > 0 && !ONLY.includes(lang.id)) {
    skipped.push(`${lang.id} (MCPD_E2E_LANGUAGES excludes it)`)
  } else if (!lang.detect()) {
    skipped.push(`${lang.id} (${lang.why})`)
  } else {
    runnable.push(lang)
  }
}

const ENVIRONMENT = {
  MCPD_ENVIRONMENT: 'staging',
  MCPD_SERVICE_NAME: 'errors-probe',
  MCPD_SERVICE_VERSION: '9.9.9',
}

/** Run one probe against one endpoint, and time it. */
async function probe(lang, endpoint, opts) {
  const env = {
    ...process.env,
    MCPD_ENDPOINT: endpoint,
    MCPD_PROJECT: PROJECT,
    MCPD_CONFORMANCE_LANG: lang.id,
    ...ENVIRONMENT,
  }
  const began = process.hrtime.bigint()
  const out = await lang.run(env, opts)
  const ms = Number((process.hrtime.bigint() - began) / 1_000_000n)
  const lines = (out.stdout || '').split('\n').map((l) => l.trim()).filter(Boolean)
  // Found anywhere, not taken from the last line. Which line it lands on is not
  // the SDK's to decide when another thread is writing to the same stream, and
  // a check that depends on the ordering passes and fails at random.
  const traceId = lines.find((l) => /^[0-9a-f]{32}$/.test(l)) ?? ''
  return { ...out, ms, lines, traceId }
}

/**
 * Every language at once, against one endpoint.
 *
 * Sequentially this suite takes longer than the conformance run it sits beside,
 * because two of the scenarios are about waiting. The probes are separate
 * processes against separate ports, so nothing here depends on the order they
 * run in. The healthy baseline is measured the same way, under the same load,
 * so the comparison between them stays fair.
 */
function probeAll(endpoint, opts) {
  return Promise.all(runnable.map(async (lang) => [lang, await probe(lang, endpoint, opts)]))
}

/* --------------------------------------------------------------- baseline -- */

/**
 * Every probe against a healthy daemon first.
 *
 * Two jobs. It proves the probe works in this environment, so a failure below
 * is about the hostile server and not about a toolchain. And it measures how
 * long the probe takes when everything is fine, which is what the bound on the
 * hostile runs is measured against. A fixed number of milliseconds would be a
 * fact about the machine rather than about the SDK.
 */
const healthy = {}
for (const [lang, out] of await probeAll(open.endpoint)) {
  healthy[lang.id] = out.ms
  check(lang.id, lang.id, 'the probe runs against a healthy daemon',
    out.status === 0 && /^[0-9a-f]{32}$/.test(out.traceId),
    `exit ${out.status} in ${out.ms} ms, last line ${JSON.stringify(out.traceId.slice(0, 60))}\n`
    + `      ${(out.stderr || '').slice(0, 300)}`)
}

/**
 * How much longer a probe may take when the server ANSWERS badly.
 *
 * Refusing a connection, returning 500, hanging up and rejecting a batch all
 * resolve at once. An SDK that takes twenty seconds longer than the healthy run
 * to handle an immediate answer is retrying in the agent's own time, which is
 * what B2 forbids.
 */
const SLACK_MS = Number(process.env.MCPD_E2E_SLACK_MS ?? 20_000)

/**
 * The ceiling for a server that never answers at all.
 *
 * This one is a different claim. An SDK is allowed to wait in `shutdown()`,
 * which flushes (B5), and against a black hole that wait is its request timeout
 * plus whatever retries it makes. What is not allowed is waiting for ever: a
 * serverless function that never returns because its telemetry never flushed is
 * the outage the contract exists to prevent. The measured figures are printed
 * on every run, so the number this bounds is visible rather than assumed.
 */
const BLACK_HOLE_CEILING_MS = Number(process.env.MCPD_E2E_BLACK_HOLE_MS ?? 60_000)

/* -------------------------------------------------------------- scenarios -- */

const scenarios = [
  { id: 'refused', why: 'nothing is listening', endpoint: `http://127.0.0.1:${refusedPort}` },
  // Not the same failure as a refused connection, and the difference is where
  // it happens: a name that does not resolve fails inside the client's own
  // lookup, which in several HTTP libraries is the one call that throws
  // synchronously, straight into the agent's call stack. `.invalid` is reserved
  // by RFC 2606 and resolves nowhere, so this needs no network at all.
  { id: 'no such host', why: 'the endpoint does not resolve',
    endpoint: 'http://mcpd-e2e.invalid:9999' },
  { id: '500', why: 'the daemon answers every batch with an error', server: 'status500' },
  { id: 'hangup', why: 'the daemon drops the connection mid-response', server: 'hangup' },
  // The one scenario the control survives, and the reason is the point of the
  // control: `probe.sh` posts and never reads the reply, so a 200 with a body
  // it cannot parse is a success to it. An SDK does read the reply, which is
  // where a JSON parser throws into the agent's own call stack.
  { id: 'garbage', why: 'the daemon answers 200 with a body that is not JSON', server: 'garbage',
    controlSurvives: true },
  { id: '413', why: 'the batch is larger than the daemon accepts', server: 'toolarge' },
  { id: 'bad key', why: 'a keyed daemon refuses the batch', endpoint: () => keyed.endpoint },
  { id: 'black hole', why: 'the daemon accepts the batch and never answers', server: 'blackhole',
    ceiling: true },
]

const table = []

for (const scenario of scenarios) {
  // On stderr, and unconditional. Two of these scenarios are about waiting, so
  // the suite is silent for half a minute at a time and a reader watching a CI
  // log has no way to tell that from a hang.
  console.error(`  ${scenario.id}: ${scenario.why}`)
  const host = scenario.server ? await hostile(scenario.server) : null
  const endpoint = host
    ? host.url
    : typeof scenario.endpoint === 'function'
      ? scenario.endpoint()
      : scenario.endpoint

  // `curl` has no client library and no timeout of its own, so against a server
  // that never answers it waits until the harness reclaims it. That is the
  // control failing, which is the point, but it should not cost the suite more
  // than the ceiling the SDKs are held to.
  const timeout = scenario.ceiling ? BLACK_HOLE_CEILING_MS + 15_000 : 60_000
  for (const [lang, out] of await probeAll(endpoint, { timeout })) {
    table.push({ scenario: scenario.id, lang: lang.id, status: out.status, ms: out.ms })
    const where = `${lang.id} / ${scenario.id}`

    if (lang.id === 'curl') {
      // The control. `probe.sh` is `curl -sf` under `set -e`, so it has no SDK
      // to swallow anything. Where the raw probe fails and every SDK does not,
      // the difference is the SDK doing its job. Where the raw probe would pass
      // too, the scenario proves nothing about the SDK, so it is named here
      // rather than left to look like a check.
      if (scenario.controlSurvives) {
        check(where, lang.id, 'the raw HTTP probe has nothing to notice, so this one is about the SDKs',
          out.status === 0, `exit ${out.status} in ${out.ms} ms`)
      } else {
        check(where, lang.id, 'the raw HTTP probe fails, which is what makes this scenario hostile',
          out.status !== 0, `exit ${out.status} in ${out.ms} ms`)
      }
      continue
    }

    check(where, lang.id, 'the agent finishes anyway (B1: instrumentation never throws)',
      out.status === 0,
      `exit ${out.status} in ${out.ms} ms\n      ${(out.stderr || out.stdout || '').slice(0, 400)}`)
    check(where, lang.id, 'the run still has its ids (B6)', Boolean(out.traceId),
      `no 32-hex line on stdout; it printed ${JSON.stringify(out.lines.join(' | ').slice(0, 160))}`)
    // Two claims, deliberately apart. The one above is that the agent still got
    // its ids. This one is that the SDK did not write over the agent's own
    // output while it was failing, which is a different promise and the one
    // that is easy to break by reaching for the language's default logger.
    check(where, lang.id, 'the SDK keeps its diagnostics off the agent\'s stdout',
      out.lines.length === 1,
      `${out.lines.length} line(s) on stdout: ${JSON.stringify(out.lines.join(' | ').slice(0, 240))}`)
    if (scenario.ceiling) {
      check(where, lang.id, 'shutdown returns even when nothing ever answers (B5)',
        out.ms < BLACK_HOLE_CEILING_MS,
        `${out.ms} ms, over the ${BLACK_HOLE_CEILING_MS} ms ceiling`)
    } else {
      check(where, lang.id, 'an immediate answer costs the agent nothing extra (B2)',
        out.ms <= healthy[lang.id] + SLACK_MS,
        `${out.ms} ms against ${healthy[lang.id]} ms healthy, `
        + `which is ${out.ms - healthy[lang.id]} ms more than the ${SLACK_MS} ms allowed`)
    }
  }

  if (host) {
    check(scenario.id, scenario.id, 'the SDKs did reach the broken server',
      host.state.requests > 0,
      `${host.state.requests} request(s) arrived, so nothing was actually tested`)
    for (const held of host.state.held ?? []) held.destroy()
    host.server.close()
  }
}

/* ------------------------------------------------------- the daemon's side -- */

/**
 * The same question from the other end: what a hostile client does to mcpd.
 *
 * The status code matters less than the last check in each group. A daemon that
 * answers 413 and then stops serving has still been taken down by one request.
 */
async function ingest(endpoint, body, headers = {}) {
  let res
  try {
    res = await fetch(`${endpoint}/v1/ingest`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...headers },
      body,
    })
  } catch (err) {
    // A body over the limit is refused while it is still being sent, so the
    // daemon answers and closes before the client has finished writing and the
    // write fails with EPIPE. That is a refusal, not a crash, and the check
    // that matters is the one below: the daemon is still serving afterwards.
    return { status: 0, text: `the connection was closed: ${err.cause?.code ?? err.message}`, json: null }
  }
  const text = await res.text()
  let json
  try {
    json = JSON.parse(text)
  } catch {
    json = null
  }
  return { status: res.status, text, json }
}

const hex = (n) =>
  [...crypto.getRandomValues(new Uint8Array(n))].map((b) => b.toString(16).padStart(2, '0')).join('')

/** One valid span, so a good batch can be sent before and after the hostile ones. */
function goodBatch() {
  const traceId = hex(16)
  const now = new Date().toISOString()
  return {
    traceId,
    body: JSON.stringify({
      spans: [{
        trace_id: traceId,
        span_id: hex(8),
        parent_span_id: null,
        name: 'still_serving',
        kind: 'agent_run',
        status: 'ok',
        start_time: now,
        end_time: now,
        duration_ns: 1_000_000,
        is_root: true,
        is_event: false,
        received_at: '1970-01-01T00:00:00.000Z',
        correlation: { project: PROJECT },
      }],
    }),
  }
}

/** A body over the daemon's 32 MiB limit, built without holding two copies. */
function oversized() {
  const padding = 'x'.repeat(1024 * 1024)
  const parts = ['{"spans":[],"logs":[],"metrics":[],"scores":[],"feedback":[],"note":"']
  for (let i = 0; i < 33; i++) parts.push(padding)
  parts.push('"}')
  return parts.join('')
}

const big = await ingest(open.endpoint, oversized())
check('daemon', 'daemon', 'an oversized batch is refused, not accepted',
  big.status === 413 || big.status === 400 || big.status === 0,
  `${big.status} ${big.text.slice(0, 200)}`)

const malformed = await ingest(open.endpoint, '{"spans": [ {"trace_id": ')
check('daemon', 'daemon', 'malformed JSON is a client error', malformed.status === 400,
  `${malformed.status} ${malformed.text.slice(0, 200)}`)
check('daemon', 'daemon', 'the refusal says what was wrong with the body',
  /json|parse|expect|eof/i.test(malformed.text), malformed.text.slice(0, 300))

// An item the daemon cannot store must be named, not dropped. A batch that is
// half rejected and reported as accepted is the worst of both: the sender stops
// retrying and the data is not there.
const invalid = await ingest(open.endpoint, JSON.stringify({
  spans: [{
    trace_id: 'not-a-trace-id',
    span_id: 'nope',
    name: 'bad_span',
    kind: 'agent_run',
    status: 'ok',
    start_time: new Date().toISOString(),
    end_time: new Date().toISOString(),
    duration_ns: 1,
    is_root: true,
    is_event: false,
    received_at: '1970-01-01T00:00:00.000Z',
    correlation: { project: PROJECT },
  }],
}))
check('daemon', 'daemon', 'an unstorable item is not silently accepted',
  invalid.status === 400 || (invalid.json?.rejected ?? 0) > 0,
  `${invalid.status} ${invalid.text.slice(0, 300)}`)
check('daemon', 'daemon', 'the refusal names what it could not store',
  /trace_id|span_id|hex|invalid/i.test(invalid.text), invalid.text.slice(0, 300))

const good = goodBatch()
const after = await ingest(open.endpoint, good.body)
check('daemon', 'daemon', 'the daemon still accepts a good batch after all of that',
  after.status === 200 || after.status === 202, `${after.status} ${after.text.slice(0, 200)}`)
check('daemon', 'daemon', 'the daemon is still healthy', (await fetch(`${open.endpoint}/healthz`)).ok)

/* --- the keyed daemon --- */

const noKey = await ingest(keyed.endpoint, goodBatch().body)
check('daemon', 'daemon', 'a keyed daemon refuses a batch with no key',
  noKey.status === 401 || noKey.status === 403, `${noKey.status} ${noKey.text.slice(0, 200)}`)

const wrongKey = await ingest(keyed.endpoint, goodBatch().body, { authorization: 'Bearer wrong' })
check('daemon', 'daemon', 'a keyed daemon refuses the wrong key',
  wrongKey.status === 401 || wrongKey.status === 403,
  `${wrongKey.status} ${wrongKey.text.slice(0, 200)}`)

// A refusal that says whether the project exists is a way to enumerate other
// people's projects one guess at a time.
const unknownProject = JSON.parse(goodBatch().body)
unknownProject.spans[0].correlation.project = 'a-project-that-does-not-exist'
const strangerRes = await ingest(keyed.endpoint, JSON.stringify(unknownProject), {
  authorization: 'Bearer wrong',
})
check('daemon', 'daemon', 'the refusal does not say whether the project exists',
  strangerRes.status === wrongKey.status && strangerRes.text === wrongKey.text,
  `${strangerRes.status} ${strangerRes.text.slice(0, 200)} against `
  + `${wrongKey.status} ${wrongKey.text.slice(0, 200)}`)

const keyedGood = await ingest(keyed.endpoint, goodBatch().body, {
  authorization: `Bearer ${INGEST_KEY}`,
})
check('daemon', 'daemon', 'the right key still works after every refusal',
  keyedGood.status === 200 || keyedGood.status === 202,
  `${keyedGood.status} ${keyedGood.text.slice(0, 200)}`)
check('daemon', 'daemon', 'the keyed daemon is still healthy', (await fetch(`${keyed.endpoint}/healthz`)).ok)

/* --------------------------------------------------- the list, kept honest -- */

for (const key of Object.keys(KNOWN_GAPS)) {
  const lang = key.slice(0, key.indexOf(':'))
  // A language that did not run says nothing about its gaps either way.
  if (!runnable.some((l) => l.id === lang)) continue
  if (!evaluated.has(key)) {
    failures.push(
      `KNOWN_GAPS names "${key}" and ${lang} ran no check by that name. Rename the entry to `
      + 'match the check, or delete it: an entry that matches no check suppresses nothing.',
    )
  } else if (!firedGaps.has(key)) {
    failures.push(
      `KNOWN_GAPS still names "${key}", and that check passed in every scenario. Delete the `
      + 'entry from tests/e2e/errors.mjs. Deleting it is how the fix is proved: the check is a '
      + 'failure again the moment the behaviour comes back.',
    )
  }
}

/* ---------------------------------------------------------------- report -- */

console.log(`\nerror paths: ${runnable.length} language(s), ${checks} checks`)
for (const lang of runnable) {
  const runs = table.filter((r) => r.lang === lang.id)
  const worst = runs.reduce((a, b) => (b.ms > a.ms ? b : a), runs[0])
  console.log(
    `  ok    ${lang.id.padEnd(11)} healthy ${String(healthy[lang.id]).padStart(6)} ms, `
    + `slowest broken run ${String(worst?.ms ?? 0).padStart(6)} ms (${worst?.scenario})`,
  )
}
for (const s of skipped) console.log(`  skip  ${s}`)

if (gaps.length) {
  console.log(`\n${gaps.length} known difference(s) between languages:\n`)
  for (const g of gaps) console.log(`  !  ${g}`)
}

if (skipped.length && process.env.MCPD_REQUIRE_ALL_LANGUAGES === '1') {
  failures.push(`MCPD_REQUIRE_ALL_LANGUAGES is set and these were skipped: ${skipped.join(', ')}`)
}

if (runnable.length === 0) {
  failures.push('no probe ran, so nothing here was tested')
}

if (failures.length) {
  console.error(`\n${failures.length} failure(s):\n`)
  for (const f of failures) console.error(`  x ${f}`)
  stop()
  process.exit(1)
}

console.log('\nevery SDK survived a broken daemon, and the daemon survived a broken client.')
stop()
