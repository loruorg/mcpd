#!/usr/bin/env node
/**
 * The MCP server, over stdio, against a live daemon.
 *
 * `mcpd mcp` is what lets an agent read its own traces, so the thing that has
 * to be true is not that a function returns a string: it is that a real child
 * process, spoken to in JSON-RPC on stdin and stdout, answers a real deployed
 * daemon's data. Everything here is checked end to end for that reason.
 *
 * Three properties this suite exists to hold:
 *
 * 1. **stdout carries JSON-RPC and nothing else.** One stray log line and every
 *    MCP client in the world drops the session. Every line the child prints is
 *    parsed, and an unparseable one is a failure, not a warning.
 * 2. **Tool results are readable text.** A run comes back as a waterfall with
 *    prompts rendered as prose, not as span objects. The assertions look for
 *    the readable form, so a regression to a JSON dump fails here.
 * 3. **It obeys the key scopes.** The MCP server is an HTTP client, so a key
 *    scoped to one project must not read another through it. The daemon is
 *    started with two projects and two keys, and the crossing is attempted.
 *
 *     node tests/e2e/mcp.mjs
 */

import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'

const ROOT = path.resolve(import.meta.dirname, '..', '..')

/**
 * A port the operating system says is free.
 *
 * Never a fixed number. This suite used to bind 47620, and a leftover daemon on
 * that port made every run pass while its own daemon never started: the harness
 * spoke to the stale process instead, its own database stayed zero bytes, and
 * 76 checks went green having tested nobody's build in particular.
 */
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

const PORT = Number(process.env.MCPD_E2E_MCP_PORT) || (await freePort())
const ENDPOINT = `http://127.0.0.1:${PORT}`

const PROJECT = 'mcp-e2e'
const OTHER = 'other-team'
const READ_KEY = 'read-mcp-e2e'
const INGEST_KEY = 'ingest-mcp-e2e'
const OTHER_INGEST_KEY = 'ingest-other-team'

const bin = ['target/debug/mcpd', 'target/release/mcpd']
  .map((p) => path.join(ROOT, p))
  .find(existsSync)

if (!bin) {
  console.error('No mcpd binary. Run `cargo build -p mcpd` first.')
  process.exit(2)
}

// The subcommand has to exist before anything else is worth trying. Without
// this the suite spends twenty seconds waiting for a handshake from a process
// clap already killed, and reports a timeout instead of the real reason.
if (spawnSync(bin, ['mcp', '--help'], { encoding: 'utf8' }).status !== 0) {
  console.error(
    'This mcpd has no `mcp` subcommand. Add `mod mcp;` and the Mcp variant to\n' +
      'crates/mcpd/src/main.rs, then rebuild with `cargo build -p mcpd`.',
  )
  process.exit(2)
}

/* ------------------------------------------------------------- assertions -- */

const failures = []
let checks = 0

function check(what, condition, detail = '') {
  checks++
  if (!condition) failures.push(`${what}${detail ? `\n      ${detail}` : ''}`)
}

/** Assert that a rendered tool result contains a piece of text. */
function contains(what, text, needle) {
  check(what, text.includes(needle), `expected to find ${JSON.stringify(needle)} in:\n${indent(text)}`)
}

function absent(what, text, needle) {
  check(what, !text.includes(needle), `did not expect ${JSON.stringify(needle)} in:\n${indent(text)}`)
}

function indent(text) {
  return String(text)
    .split('\n')
    .map((l) => `      | ${l}`)
    .join('\n')
}

/* ------------------------------------------------------------- the daemon -- */

// Named from the pid AND a per-process nanosecond stamp. Pids are recycled, and
// a recycled one reopens the previous run's database, so every count is off by
// a whole prior suite.
const stamp = `${process.pid}-${process.hrtime.bigint()}`
const dataDir = mkdtempSync(path.join(tmpdir(), `mcpd-mcp-e2e-${stamp}-`))
const configPath = path.join(dataDir, 'mcpd.json')

// No global keys at all: every key here is scoped to exactly one project, which
// is the arrangement that makes the crossing test meaningful. A top-level key
// would reach everything and prove nothing.
writeFileSync(
  configPath,
  JSON.stringify({
    projects: {
      [PROJECT]: { ingest_keys: [INGEST_KEY], read_keys: [READ_KEY] },
      [OTHER]: { ingest_keys: [OTHER_INGEST_KEY], read_keys: ['read-other-team'] },
    },
  }),
)

const daemon = spawn(
  bin,
  [
    'serve',
    '--data',
    path.join(dataDir, 'mcp-e2e.db'),
    '--listen',
    `127.0.0.1:${PORT}`,
    '--config-file',
    configPath,
    '--project',
    PROJECT,
  ],
  { stdio: 'ignore' },
)

const children = [daemon]

// The database is the whole of this suite's state. Leaving it behind fills the
// temp directory with one 800 KB database per run and leaves a stale daemon
// something to reopen.
function stop() {
  for (const c of children) c.kill()
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

// If the daemon dies, say so. Without this the suite reports a twenty-second
// handshake timeout, or worse, silently talks to whatever else is on the port.
let daemonExit = null
daemon.on('exit', (code, signal) => {
  daemonExit = signal ? `signal ${signal}` : `code ${code}`
})

async function waitForDaemon() {
  for (let i = 0; i < 100; i++) {
    if (daemonExit) return false
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
    daemonExit
      ? `The daemon exited with ${daemonExit} before it became healthy. Its data directory was ${dataDir}.`
      : `The daemon never became healthy on ${ENDPOINT}.`,
  )
  process.exit(2)
}

// The port was free when it was picked, so anything answering now that is not
// the child we started is a different process, and every check below would be
// about someone else's build.
const version = await (await fetch(`${ENDPOINT}/healthz`)).json()
if (daemonExit) {
  console.error(`The daemon exited with ${daemonExit}, but ${ENDPOINT} still answers.`)
  console.error(`Something else holds the port. Reported version ${version.version}.`)
  process.exit(2)
}

/* ---------------------------------------------------------------- the run -- */

const hex = (n) =>
  [...crypto.getRandomValues(new Uint8Array(n))].map((b) => b.toString(16).padStart(2, '0')).join('')

const TRACE = hex(16)
const ROOT_SPAN = hex(8)
const LOAD_SPAN = hex(8)
const MODEL_SPAN = hex(8)
const TOOL_SPAN = hex(8)

const started = new Date(Date.now() - 5 * 60_000)
const at = (ms) => new Date(started.getTime() + ms).toISOString()
const EPOCH = '1970-01-01T00:00:00.000Z'
const correlation = { project: PROJECT, environment: 'production', session_id: 'session-nine' }

/** One realistic failing run: a plan, a lookup, and a tool that fell over. */
const batch = {
  spans: [
    {
      trace_id: TRACE,
      span_id: ROOT_SPAN,
      parent_span_id: null,
      name: 'support_agent',
      kind: 'agent_run',
      // The root reports its own outcome, `ok`; mcpd propagates the child's
      // failure to the trace. That is what every SDK does, so it is what the
      // fixture must do, or this suite tests a shape nothing produces.
      status: 'ok',
      start_time: at(0),
      end_time: at(4210),
      duration_ns: 4_210_000_000,
      is_root: true,
      is_event: false,
      received_at: EPOCH,
      correlation,
      tags: ['checkout'],
      input: 'where is my order 41?',
    },
    {
      trace_id: TRACE,
      span_id: LOAD_SPAN,
      parent_span_id: ROOT_SPAN,
      name: 'load_cart',
      kind: 'tool_call',
      status: 'ok',
      start_time: at(4),
      end_time: at(124),
      duration_ns: 120_000_000,
      is_root: false,
      is_event: false,
      received_at: EPOCH,
      correlation,
    },
    {
      trace_id: TRACE,
      span_id: MODEL_SPAN,
      parent_span_id: ROOT_SPAN,
      name: 'plan_answer',
      kind: 'model_generation',
      status: 'ok',
      start_time: at(130),
      end_time: at(2030),
      duration_ns: 1_900_000_000,
      is_root: false,
      is_event: false,
      received_at: EPOCH,
      correlation,
      model: 'gpt-4o',
      provider: 'openai',
      usage: { input_tokens: 1180, output_tokens: 96 },
      input: {
        messages: [
          { role: 'system', content: 'You are a terse support agent.' },
          { role: 'user', content: [{ type: 'text', text: 'where is my order 41?' }] },
        ],
      },
      output: 'I will check the delivery estimate.',
    },
    {
      // A child of the model step, so the waterfall has a third level to indent.
      trace_id: TRACE,
      span_id: TOOL_SPAN,
      parent_span_id: MODEL_SPAN,
      name: 'estimate_delivery',
      kind: 'tool_call',
      status: 'error',
      start_time: at(2050),
      end_time: at(2060),
      duration_ns: 10_000_000,
      is_root: false,
      is_event: false,
      received_at: EPOCH,
      correlation,
      input: { order_id: 41 },
      error: {
        message: 'shipping service returned 503',
        kind: 'HttpError',
        retryable: true,
      },
    },
  ],
  logs: [
    {
      log_id: hex(16),
      trace_id: TRACE,
      span_id: TOOL_SPAN,
      timestamp: at(2055),
      level: 'error',
      message: 'shipping service unavailable, giving up after three attempts',
      correlation,
      received_at: EPOCH,
    },
  ],
  scores: [
    {
      score_id: hex(16),
      trace_id: TRACE,
      scorer_id: 'answer_relevance',
      source: 'offline',
      value: 0.21,
      min: 0,
      max: 1,
      reason: 'the answer never gave a delivery date',
      timestamp: at(4300),
      correlation,
    },
  ],
  feedback: [
    {
      feedback_id: hex(16),
      trace_id: TRACE,
      kind: 'thumbs',
      value: 'down',
      comment: 'never told me when it arrives',
      timestamp: at(4400),
      correlation,
    },
  ],
}

/** A run belonging to another team, which this suite's key must never see. */
const otherBatch = {
  spans: [
    {
      trace_id: hex(16),
      span_id: hex(8),
      parent_span_id: null,
      name: 'their_private_agent',
      kind: 'agent_run',
      status: 'ok',
      start_time: at(0),
      end_time: at(1000),
      duration_ns: 1_000_000_000,
      is_root: true,
      is_event: false,
      received_at: EPOCH,
      correlation: { project: OTHER },
    },
  ],
}

async function ingest(body, key) {
  const res = await fetch(`${ENDPOINT}/v1/ingest`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`ingest -> ${res.status} ${await res.text()}`)
  const result = await res.json()
  if (result.rejected) throw new Error(`ingest rejected ${result.rejected}: ${JSON.stringify(result.errors)}`)
}

await ingest(batch, INGEST_KEY)
await ingest(otherBatch, OTHER_INGEST_KEY)

/* ------------------------------------------------------------ the client -- */

/**
 * A JSON-RPC client over one child process's stdio.
 *
 * It records every line the child prints, so the suite can assert both what
 * came back and that nothing else did.
 */
function client(args) {
  const child = spawn(bin, ['mcp', ...args], { cwd: ROOT, stdio: ['pipe', 'pipe', 'pipe'] })
  children.push(child)

  const pending = new Map()
  const received = []
  const junk = []
  let stderr = ''
  let buffer = ''

  child.stderr.on('data', (c) => {
    stderr += c
  })
  child.stdout.on('data', (chunk) => {
    buffer += chunk
    let cut
    while ((cut = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, cut).trim()
      buffer = buffer.slice(cut + 1)
      if (!line) continue
      let message
      try {
        message = JSON.parse(line)
      } catch {
        junk.push(line)
        continue
      }
      received.push(message)
      const waiting = pending.get(message.id)
      if (waiting) {
        pending.delete(message.id)
        waiting(message)
      }
    }
  })

  let nextId = 1
  return {
    child,
    received,
    junk,
    get stderr() {
      return stderr
    },
    notify(method, params) {
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, params })}\n`)
    },
    raw(line) {
      child.stdin.write(`${line}\n`)
    },
    send(method, params, id = nextId++) {
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id)
          reject(new Error(`${method} never answered. stderr:\n${stderr}`))
        }, 20_000)
        pending.set(id, (message) => {
          clearTimeout(timer)
          resolve(message)
        })
        child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`)
      })
    },
    /** Call a tool and return { text, isError }. */
    async call(name, args = {}) {
      const reply = await this.send('tools/call', { name, arguments: args })
      if (reply.error) return { error: reply.error, text: '', isError: true }
      return {
        text: reply.result.content.map((c) => c.text).join('\n'),
        isError: Boolean(reply.result.isError),
      }
    },
    close() {
      child.stdin.end()
    },
  }
}

/* ------------------------------------------------------------------ main -- */

const mcp = client(['--endpoint', ENDPOINT, '--api-key', READ_KEY, '--project', PROJECT])

/* --- the handshake --- */

const init = await mcp.send('initialize', {
  protocolVersion: '2024-11-05',
  capabilities: {},
  clientInfo: { name: 'mcpd-e2e', version: '0' },
})
check('initialize echoes the version the client asked for', init.result?.protocolVersion === '2024-11-05', JSON.stringify(init))
check('initialize names the server', init.result?.serverInfo?.name === 'mcpd')
check('initialize advertises tools', Boolean(init.result?.capabilities?.tools))
check('initialize carries instructions for the agent', (init.result?.instructions || '').length > 40)

const newer = await mcp.send('initialize', { protocolVersion: '2099-01-01', capabilities: {} })
check(
  'an unsupported version is answered with a supported one',
  ['2025-06-18', '2024-11-05'].includes(newer.result?.protocolVersion),
  JSON.stringify(newer),
)

// A notification must never be answered. Sending one and then a request proves
// it: only the request's reply may arrive.
const before = mcp.received.length
mcp.notify('notifications/initialized', {})
const pong = await mcp.send('ping', {})
check('ping answers', Boolean(pong.result))
check(
  'a notification is not answered',
  mcp.received.length === before + 1,
  `${mcp.received.length - before} replies arrived where one was expected`,
)

/* --- the catalogue --- */

const list = await mcp.send('tools/list', {})
const tools = list.result?.tools ?? []
check('three tools, and no more', tools.length === 3, JSON.stringify(tools.map((t) => t.name)))
check(
  'the tools are find_runs, get_run and run_stats',
  JSON.stringify(tools.map((t) => t.name).sort()) === JSON.stringify(['find_runs', 'get_run', 'run_stats']),
)
for (const tool of tools) {
  check(`${tool.name} declares an object schema`, tool.inputSchema?.type === 'object')
  check(`${tool.name} describes itself`, (tool.description || '').length > 40)
  check(`${tool.name} is marked read-only`, tool.annotations?.readOnlyHint === true)
}

/* --- find_runs --- */

const found = await mcp.call('find_runs', { status: 'error' })
check('find_runs succeeded', !found.isError, found.text)
contains('find_runs prints the trace id, so get_run can be called next', found.text, TRACE)
contains('find_runs names the run', found.text, 'support_agent')
contains('find_runs has a column header', found.text, 'trace_id')
contains('find_runs reports the failed step count', found.text, 'failed')

const none = await mcp.call('find_runs', { status: 'ok', name: 'support_agent' })
check('a search that matches nothing is not an error', !none.isError, none.text)
contains('an empty result says how to widen the search', none.text, 'Widen the window')

const byName = await mcp.call('find_runs', { name: 'support_agent', since: '-1h', limit: 5 })
contains('find_runs filters by name', byName.text, TRACE)

const typo = await mcp.send('tools/call', { name: 'find_runs', arguments: { statuz: 'error' } })
check('a mistyped argument is refused rather than ignored', typo.error?.code === -32602, JSON.stringify(typo))
contains('the refusal names the argument', typo.error?.message ?? '', 'statuz')

/* --- get_run --- */

const run = await mcp.call('get_run', { trace_id: TRACE })
check('get_run succeeded', !run.isError, run.text)
contains('get_run identifies the run', run.text, TRACE)
contains('get_run leads with what failed', run.text, '1 step failed:')
contains('get_run names the failing step', run.text, 'estimate_delivery')
contains('get_run gives the failure reason', run.text, 'shipping service returned 503')
contains('get_run carries the error kind', run.text, 'kind=HttpError')
contains('get_run draws a waterfall', run.text, 'waterfall')
contains('the waterfall has bars', run.text, '####')
contains('the waterfall indents a child step', run.text, '  estimate_delivery')
contains('get_run reports token usage', run.text, '1,180 in / 96 out')
contains('get_run reports the model', run.text, 'model=gpt-4o (openai)')
contains('get_run renders the prompt as prose, not JSON', run.text, 'user: where is my order 41?')
contains('get_run renders the system turn too', run.text, 'system: You are a terse support agent.')
contains('get_run carries the completion', run.text, 'I will check the delivery estimate.')
contains('get_run carries the logs', run.text, 'shipping service unavailable')
contains('get_run carries the score', run.text, 'answer_relevance')
contains('get_run carries the score reason', run.text, 'never gave a delivery date')
contains('get_run carries the feedback', run.text, 'thumbs')
contains('get_run carries the run context', run.text, 'environment=production')

// The whole point of the text rendering: a four-span run must not come back as
// four span objects. A JSON dump would carry these keys.
absent('get_run does not dump span JSON', run.text, '"span_id"')
absent('get_run does not dump span JSON', run.text, '"parent_span_id"')

const quiet = await mcp.call('get_run', { trace_id: TRACE, payloads: 'none' })
check('get_run with payloads=none succeeded', !quiet.isError, quiet.text)
absent('payloads=none withholds the prompt', quiet.text, 'where is my order 41?')
contains('payloads=none still reports the failure', quiet.text, 'shipping service returned 503')
contains('payloads=none still draws the waterfall', quiet.text, 'waterfall')
check(
  'payloads=none is materially cheaper to read',
  quiet.text.length < run.text.length,
  `${quiet.text.length} vs ${run.text.length} characters`,
)

const onlyErrors = await mcp.call('get_run', { trace_id: TRACE, payloads: 'errors' })
contains('payloads=errors keeps the failing step input', onlyErrors.text, 'order_id')
absent('payloads=errors withholds the model prompt', onlyErrors.text, 'where is my order 41?')

const capped = await mcp.call('get_run', { trace_id: TRACE, max_payload_chars: 12 })
contains('a cut payload says it was cut', capped.text, 'more characters')

const missing = await mcp.call('get_run', { trace_id: 'f'.repeat(32) })
check('an unknown run is a readable tool failure, not a crash', missing.isError, missing.text)
contains('the failure explains what to do', missing.text, 'find_runs')

/* --- run_stats --- */

const stats = await mcp.call('run_stats', { since: '-1h' })
check('run_stats succeeded', !stats.isError, stats.text)
contains('run_stats counts the runs', stats.text, 'runs')
contains('run_stats counts the failures', stats.text, 'failed')
contains('run_stats reports latency', stats.text, 'p50')
contains('run_stats suggests a breakdown', stats.text, 'group_by')

const byModel = await mcp.call('run_stats', { since: '-1h', group_by: 'model' })
check('run_stats grouped succeeded', !byModel.isError, byModel.text)
contains('the breakdown names the model', byModel.text, 'gpt-4o')

const badGroup = await mcp.call('run_stats', { group_by: 'nonsense' })
check('an invalid group_by is refused', badGroup.isError, badGroup.text)
contains('the refusal lists what is allowed', badGroup.text, 'model')

/* --- the protocol's edges --- */

const unknownMethod = await mcp.send('resources/list', {})
check('an unimplemented method is method-not-found', unknownMethod.error?.code === -32601, JSON.stringify(unknownMethod))

const unknownTool = await mcp.send('tools/call', { name: 'delete_everything', arguments: {} })
check('an unknown tool is refused', unknownTool.error?.code === -32602, JSON.stringify(unknownTool))

const brokenLine = new Promise((resolve) => {
  const watch = setInterval(() => {
    const parseError = mcp.received.find((m) => m.error?.code === -32700)
    if (parseError) {
      clearInterval(watch)
      resolve(parseError)
    }
  }, 50)
  setTimeout(() => {
    clearInterval(watch)
    resolve(null)
  }, 5000)
})
mcp.raw('{not json at all')
check('a line that is not JSON is a parse error', Boolean(await brokenLine))

// Nothing but JSON-RPC may ever reach stdout.
check('stdout carried JSON-RPC and nothing else', mcp.junk.length === 0, mcp.junk.join('\n'))

mcp.close()

/* --- the key scopes --- */

// The same key, pointed at another team's project. The MCP server holds no
// authority of its own: the daemon refuses this, and the refusal has to reach
// the agent as readable text.
const crossing = client(['--endpoint', ENDPOINT, '--api-key', READ_KEY, '--project', OTHER])
await crossing.send('initialize', { protocolVersion: '2025-06-18', capabilities: {} })
const crossed = await crossing.call('find_runs', {})
check('a key scoped to one project cannot read another', crossed.isError, crossed.text)
absent('the refusal leaks no other project data', crossed.text, 'their_private_agent')
crossing.close()

const anonymous = client(['--endpoint', ENDPOINT, '--project', PROJECT])
await anonymous.send('initialize', { protocolVersion: '2025-06-18', capabilities: {} })
const refused = await anonymous.call('find_runs', {})
check('a keyed daemon refuses an MCP server with no key', refused.isError, refused.text)
contains('the refusal says how to supply one', refused.text, 'MCPD_API_KEY')
anonymous.close()

/* ---------------------------------------------------------------- report -- */

console.log(`\nmcp: ${checks} checks`)

if (failures.length) {
  console.error(`\n${failures.length} failure(s):\n`)
  for (const f of failures) console.error(`  x ${f}`)
  stop()
  process.exit(1)
}

console.log('the MCP server answered every tool over stdio, and obeyed the key scopes.')
// Explicit, not left to the `exit` handler: an open pipe to a live child keeps
// the event loop alive, so the suite would sit there after its last check.
stop()
