#!/usr/bin/env node
/**
 * Cross-language conformance for durable runs.
 *
 * Every SDK's worker is unit-tested on its own against a fake daemon, which
 * proves each does what its authors meant and proves nothing about the
 * property that matters: that all six serve the SAME run the same way. This
 * suite is the only thing that can check that.
 *
 * Each probe builds the canonical workflow through its own builder, registers
 * it, starts a run, publishes the approval event before the wait exists (so
 * the event is parked and taken when the wait opens), and serves the run to
 * its end with its own worker: `started`, a nested span under the daemon's
 * step span, usage the daemon prices, a retryable failure the daemon retries,
 * an at_most_once step, and the run's output template. The harness then reads
 * the definition, the run, its journal and its trace back through the HTTP
 * API and asserts they are identical across languages and equal to what
 * canonical.json pins.
 *
 * A language whose toolchain is absent is REPORTED as skipped and counted,
 * never passed over quietly. `MCPD_REQUIRE_ALL_LANGUAGES=1` turns a skip into
 * a failure; `MCPD_E2E_LANGUAGES=typescript,python` narrows a run.
 */

import { spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { ROOT, RUN_PROBES, freePort, languageFilter, mcpdBinary } from './languages.mjs'

const CANONICAL = JSON.parse(readFileSync(path.join(ROOT, 'tests/e2e/probes/runs/canonical.json'), 'utf8'))

const bin = mcpdBinary()
if (!bin) {
  console.error('No mcpd binary. Run `cargo build -p mcpd` first.')
  process.exit(2)
}

const PORT = Number(process.env.MCPD_E2E_PORT) || (await freePort())
const ENDPOINT = `http://127.0.0.1:${PORT}`
const ONLY = languageFilter()
const DEBUG = Boolean(process.env.MCPD_E2E_DEBUG)

const failures = []
const skipped = []
let checks = 0
const perLanguage = {}

function check(lang, what, condition, detail = '') {
  checks++
  perLanguage[lang] = (perLanguage[lang] ?? 0) + 1
  if (condition) return
  failures.push(`${lang}: ${what}${detail ? `, got ${detail}` : ''}`)
}

function same(a, b) {
  return JSON.stringify(sorted(a)) === JSON.stringify(sorted(b))
}

function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted)
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((k) => [k, sorted(value[k])]))
  }
  return value
}

function show(value) {
  const text = JSON.stringify(sorted(value))
  return text.length > 400 ? text.slice(0, 400) + '...' : text
}

/* --------------------------------------------------------------- reading -- */

async function api(project, pathname, query = {}) {
  const url = new URL(`${ENDPOINT}/v1/api${pathname}`)
  for (const [k, v] of Object.entries({ project, ...query })) {
    if (v !== undefined) url.searchParams.set(k, String(v))
  }
  const res = await fetch(url)
  if (!res.ok) throw new Error(`${url.pathname}${url.search} -> ${res.status} ${await res.text()}`)
  return res.json()
}

async function readBack(project, probe) {
  const definition = await api(project, `/definitions/${CANONICAL.definition_id}`)
  const detail = await api(project, `/runs/${probe.run_id}`)
  const events = []
  let after = -1
  for (;;) {
    const page = await api(project, `/runs/${probe.run_id}/events`, { after, limit: 500 })
    events.push(...page.events)
    if (page.events.length) after = page.events[page.events.length - 1].seq
    if (!page.has_more) break
  }
  // The daemon writes the run's own spans through an outbox drained on its
  // own clock; the probe's own span arrives through ingest. Wait for both.
  let trace
  for (let i = 0; i < 40; i++) {
    try {
      trace = await api(project, `/traces/${probe.trace_id}`)
      if (trace.spans.some((s) => s.name === 'model_call') && trace.spans.length >= 4) break
    } catch {
      trace = undefined
    }
    await new Promise((r) => setTimeout(r, 250))
  }
  return { definition, detail, events, trace }
}

/** The part of what was read back that every language must agree on. */
function shape(read) {
  const names = new Map(read.trace?.spans.map((s) => [s.span_id, s.name]) ?? [])
  return {
    definition: read.definition.body,
    status: read.detail.run.status,
    output: read.detail.run.output,
    state: read.detail.run.state,
    stepsUsed: read.detail.run.steps_used,
    attempts: [...read.detail.attempts]
      .sort((a, b) => a.step_key.localeCompare(b.step_key) || a.attempt - b.attempt)
      .map((a) => ({
        step_key: a.step_key,
        attempt: a.attempt,
        kind: a.kind,
        effect: a.effect,
        status: a.status,
        input: a.input ?? null,
        output: a.output ?? null,
        error: a.error ? { class: a.error.class, message: a.error.message } : null,
        usage: a.usage ? { input_tokens: a.usage.input_tokens ?? null, output_tokens: a.usage.output_tokens ?? null } : null,
        cost_usd: a.cost_usd ?? null,
      })),
    events: read.events.map((e) => e.kind),
    spans: (read.trace?.spans ?? [])
      .map((s) => ({
        name: s.name,
        kind: s.kind,
        parent: s.parent_span_id ? (names.get(s.parent_span_id) ?? 'unknown') : null,
        status: s.status,
        model: s.model ?? null,
        usage: s.usage ? { input_tokens: s.usage.input_tokens ?? null, output_tokens: s.usage.output_tokens ?? null } : null,
        // Everything the run put on the span. The project is the language's
        // own and the run id is this run's, so both are left out.
        correlation: Object.fromEntries(
          Object.entries(s.correlation ?? {}).filter(([k]) => k !== 'project' && k !== 'run_id'),
        ),
      }))
      .sort((a, b) => `${a.name}/${a.parent}`.localeCompare(`${b.name}/${b.parent}`)),
    publish: read.probe.publish,
  }
}

/* ----------------------------------------------------------------- checks -- */

function pinned(lang, read) {
  const { definition, detail, events, trace, probe } = read
  const run = detail.run
  check(lang, 'the probe reports the run succeeded', probe.status === 'succeeded', probe.status)
  check(lang, 'the definition body is exactly the canonical document', same(definition.body, CANONICAL.definition_body), show(definition.body))
  check(lang, 'the definition is version 1', definition.version === 1, String(definition.version))
  check(lang, 'the run succeeded', run.status === 'succeeded', `${run.status} ${run.error ?? ''}`)
  check(lang, 'the run pinned version 1', run.definition_version === 1, String(run.definition_version))
  check(lang, 'the output template rendered', same(run.output, CANONICAL.output), show(run.output))
  check(lang, 'the state the body merged is on the run', same(run.state, CANONICAL.state), show(run.state))
  check(lang, 'the run carries the session it was started with', run.correlation?.session_id === CANONICAL.correlation.session_id, show(run.correlation))
  check(lang, 'the run carries the user it was started with', run.correlation?.user_id === CANONICAL.correlation.user_id, show(run.correlation))
  check(lang, 'the first publish was parked, the second a duplicate', same(probe.publish, ['parked', 'duplicate']), show(probe.publish))

  const attempt = (key, n) => detail.attempts.find((a) => a.step_key === key && a.attempt === n)
  const classify = attempt('classify', 0)
  check(lang, 'classify succeeded as a task', classify?.status === 'succeeded' && classify?.kind === 'task', show(classify))
  check(lang, 'classify output', same(classify?.output, { kind: 'question' }), show(classify?.output))
  const draft = attempt('draft', 0)
  check(lang, 'draft succeeded as an agent step', draft?.status === 'succeeded' && draft?.kind === 'agent', show(draft))
  check(lang, 'draft got its rendered input', same(draft?.input, { kind: 'question' }), show(draft?.input))
  check(lang, 'draft output', draft?.output === CANONICAL.output.answer, show(draft?.output))
  check(
    lang,
    'draft usage reached the daemon',
    draft?.usage?.input_tokens === CANONICAL.model.input_tokens && draft?.usage?.output_tokens === CANONICAL.model.output_tokens,
    show(draft?.usage),
  )
  const approval = attempt('approval', 0)
  check(lang, 'the wait took the parked event', approval?.status === 'succeeded' && approval?.kind === 'wait', show(approval))
  check(lang, 'the wait output is the event payload', same(approval?.output, CANONICAL.event.payload), show(approval?.output))
  const first = attempt('send', 0)
  check(lang, 'send attempt 0 failed retryable', first?.status === 'failed' && first?.error?.class === 'retryable', show(first))
  check(lang, 'send attempt 0 carries the message', first?.error?.message === 'relay busy', show(first?.error))
  const second = attempt('send', 1)
  check(lang, 'send attempt 1 succeeded', second?.status === 'succeeded', show(second))
  check(lang, 'send attempt 1 got both templates rendered', same(second?.input, { text: CANONICAL.output.answer, by: 'ops' }), show(second?.input))
  check(lang, 'send output', same(second?.output, { sent: true, by: 'ops' }), show(second?.output))
  check(lang, 'send is at_most_once', second?.effect === 'at_most_once', show(second?.effect))
  for (const a of detail.attempts.filter((a) => a.kind !== 'wait')) {
    check(lang, `${a.step_key}[${a.attempt}] records this probe as the lease owner`, a.lease_owner === `probe-${lang}`, String(a.lease_owner))
  }

  check(lang, 'the journal starts with run_started', events[0]?.kind === 'run_started', events[0]?.kind)
  check(lang, 'the journal ends with run_ended', events[events.length - 1]?.kind === 'run_ended', events[events.length - 1]?.kind)
  check(lang, 'the journal records the retryable failure', events.some((e) => e.kind === 'step_failed'))
  check(lang, 'the journal has no gaps', events.every((e, i) => e.seq === i), show(events.map((e) => e.seq)))

  check(lang, 'the trace was read back', Boolean(trace))
  if (!trace) return
  const model = trace.spans.find((s) => s.name === 'model_call')
  check(lang, "the body's span reached the run's trace", Boolean(model))
  check(lang, "the body's span is a model generation", model?.kind === 'model_generation', model?.kind)
  check(lang, "the body's span sits under the draft step's span", model?.parent_span_id === draft?.span_id, `${model?.parent_span_id} vs ${draft?.span_id}`)
  check(lang, "the body's span names the model", model?.model === CANONICAL.model.name, model?.model)
  check(
    lang,
    "the body's span carries the run's session",
    model?.correlation?.session_id === CANONICAL.correlation.session_id,
    show(model?.correlation),
  )
  // The daemon pseudonymises subjects on every span it stores, so the user
  // arrives as a digest; the run store keeps the id as given. Every language
  // must carry one, and the cross-language comparison says they carry the same.
  check(lang, "the body's span carries the run's user", typeof model?.correlation?.user_id === 'string' && model.correlation.user_id.startsWith('sha256:'), show(model?.correlation))
  const root = trace.spans.find((s) => !s.parent_span_id)
  check(lang, 'the trace has one root, the run', Boolean(root) && trace.spans.filter((s) => !s.parent_span_id).length === 1, show(trace.spans.map((s) => s.name)))
}

/* ------------------------------------------------------------------ main -- */

const dataDir = mkdtempSync(path.join(tmpdir(), 'mcpd-runs-e2e-'))
const daemon = spawn(bin, ['serve', '--data', path.join(dataDir, 'e2e.db'), '--listen', `127.0.0.1:${PORT}`], {
  stdio: ['ignore', DEBUG ? 'inherit' : 'ignore', 'pipe'],
})
let daemonErr = ''
daemon.stderr.on('data', (d) => {
  daemonErr += d
})

async function waitForDaemon() {
  for (let i = 0; i < 100; i++) {
    try {
      const res = await fetch(`${ENDPOINT}/healthz`)
      if (res.ok) return
    } catch {}
    await new Promise((r) => setTimeout(r, 100))
  }
  throw new Error(`daemon did not come up on ${ENDPOINT}\n${daemonErr}`)
}

const results = {}
try {
  await waitForDaemon()
  for (const probe of RUN_PROBES) {
    if (ONLY.length > 0 && !ONLY.includes(probe.id)) {
      skipped.push(`${probe.id}: left out by MCPD_E2E_LANGUAGES`)
      continue
    }
    if (!probe.detect()) {
      skipped.push(`${probe.id}: ${probe.why}`)
      continue
    }
    const project = `conformance-runs-${probe.id}`
    const started = Date.now()
    const out = await probe.run({ ...process.env, MCPD_ENDPOINT: ENDPOINT, MCPD_PROJECT: project }, { timeout: 600_000 })
    const seconds = ((Date.now() - started) / 1000).toFixed(1)
    if (DEBUG) console.log(`--- ${probe.id} (${seconds}s)\n${out.stdout}\n${out.stderr}`)
    check(probe.id, 'the probe exited 0', out.status === 0, `${out.status}\n${out.stderr.slice(-2000)}`)
    if (out.status !== 0) continue
    const last = out.stdout.trim().split('\n').pop()
    let parsed
    try {
      parsed = JSON.parse(last)
    } catch {
      check(probe.id, 'the probe printed its run as JSON', false, last)
      continue
    }
    const read = await readBack(project, parsed)
    read.probe = parsed
    results[probe.id] = read
    pinned(probe.id, read)
    console.log(`  ${probe.id.padEnd(11)} ${seconds}s`)
  }

  const langs = Object.keys(results)
  if (langs.length > 1) {
    const ref = langs[0]
    const reference = shape(results[ref])
    for (const lang of langs.slice(1)) {
      const mine = shape(results[lang])
      for (const field of Object.keys(reference)) {
        check(lang, `same ${field} as ${ref}`, same(mine[field], reference[field]), `\n      ${lang}: ${show(mine[field])}\n      ${ref}: ${show(reference[field])}`)
      }
    }
  }
} finally {
  daemon.kill('SIGTERM')
  rmSync(dataDir, { recursive: true, force: true })
}

console.log(`\n${checks} checks across ${Object.keys(perLanguage).length} language(s)`)
for (const [lang, n] of Object.entries(perLanguage)) console.log(`  ${lang.padEnd(11)} ${n}`)
if (skipped.length) {
  console.log(`\n${skipped.length} language(s) not run:`)
  for (const s of skipped) console.log(`  ${s}`)
}
if (failures.length) {
  console.log(`\n${failures.length} failure(s):`)
  for (const f of failures) console.log(`  ${f}`)
}
const requireAll = Boolean(process.env.MCPD_REQUIRE_ALL_LANGUAGES)
if (failures.length || (requireAll && skipped.length)) {
  if (requireAll && skipped.length) console.log('\nMCPD_REQUIRE_ALL_LANGUAGES is set, so a language not run is a failure.')
  process.exit(1)
}
console.log('\nevery language served the same run')
