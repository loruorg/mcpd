#!/usr/bin/env node
/**
 * Proof that the conformance checks can fail.
 *
 * Seven tests in this repository have reported success without testing
 * anything, and every one of them would have been caught by breaking the thing
 * the check guards once and watching it go red. That habit is worth having; a
 * habit nobody can repeat is worth less than one anybody can run.
 *
 * So each case below names a fault `run.mjs` can inject into every batch on the
 * way to the daemon, and the checks that fault must break. The suite is run
 * once per fault. A check that stays green under the corruption it exists to
 * catch is reported here as what it is: not a check.
 *
 *     node tests/e2e/mutations.mjs
 *     MCPD_E2E_MUTATIONS=flat-trace node tests/e2e/mutations.mjs
 *
 * It drives two languages by default, because the fault is applied to every
 * batch and one language proves the check as well as seven do. Set
 * `MCPD_E2E_LANGUAGES` to choose them.
 */

import { spawn } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'

import { LANGUAGES, ROOT } from './languages.mjs'

/**
 * Each fault, and the checks it must turn red.
 *
 * The check names are the strings `run.mjs` passes to `check()`. A rename there
 * fails here, which is the point: this file is the reason those strings cannot
 * quietly stop meaning anything.
 */
const CASES = [
  {
    fault: 'flat-trace',
    breaks: ['the run has one root, and every other step hangs off it'],
  },
  { fault: 'no-failure', breaks: ['a failed step makes the whole run an error'] },
  { fault: 'error-moves', breaks: ['the failure lands on the step that failed'] },
  { fault: 'model-renamed', breaks: ['the model call records its model'] },
  {
    fault: 'usage-drift',
    breaks: ['token usage survives the wire', 'the run costs what its steps cost'],
  },
  { fault: 'metric-unlinked', breaks: ['the metric points back at the run that made it'] },
  { fault: 'score-rescaled', breaks: ['both scores land with their scale'] },
  { fault: 'score-devalued', breaks: ['a score is findable by its value'] },
  // Found by running this file: the provider was compared between languages and
  // asserted nowhere, so a fault every language shared broke nothing.
  { fault: 'provider-renamed', breaks: ['the model call records its provider'] },
]

const only = (process.env.MCPD_E2E_MUTATIONS ?? '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean)
const unknown = only.filter((f) => !CASES.some((c) => c.fault === f))
if (unknown.length > 0) {
  console.error(`MCPD_E2E_MUTATIONS names ${unknown.join(', ')}, which is no fault here.`)
  process.exit(2)
}
const cases = only.length > 0 ? CASES.filter((c) => only.includes(c.fault)) : CASES

/**
 * Two languages, so the cross-language comparison still runs.
 *
 * Chosen from the ones whose toolchain is present rather than named, because a
 * fixed pair on a machine missing one of them would run one language and report
 * every fault as uncaught.
 */
const languages = process.env.MCPD_E2E_LANGUAGES
  ?? LANGUAGES.filter((l) => l.detect()).slice(0, 2).map((l) => l.id).join(',')
if (languages.split(',').filter(Boolean).length < 2) {
  console.error(`Only ${languages || 'no'} language is available. This needs two.`)
  process.exit(2)
}

function runSuite(fault) {
  return new Promise((resolve) => {
    const child = spawn('node', [path.join(ROOT, 'tests/e2e/run.mjs')], {
      cwd: ROOT,
      env: { ...process.env, MCPD_E2E_FAULT: fault, MCPD_E2E_LANGUAGES: languages },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (d) => { out += d })
    child.stderr.on('data', (d) => { out += d })
    child.on('close', (status) => resolve({ status: status ?? 1, out }))
  })
}

const failures = []
const rows = []

console.error(`driving ${languages}\n`)

for (const { fault, breaks } of cases) {
  process.stderr.write(`  ${fault.padEnd(16)}`)
  const { status, out } = await runSuite(fault)
  // A failure is a line the suite printed under "failure(s)"; a known gap is a
  // line printed under "known difference(s)". A check swallowed as a known gap
  // has NOT failed the run, so it is not proof of anything and is named as
  // such rather than counted.
  const failed = out.split('\n').filter((l) => l.startsWith('  x '))
  const known = out.split('\n').filter((l) => l.startsWith('  !  '))

  if (status === 0) {
    failures.push(`${fault}: the suite passed with the fault injected, which cannot be right`)
  }
  for (const what of breaks) {
    const red = failed.some((l) => l.includes(`: ${what}`))
    const swallowed = known.some((l) => l.includes(`: ${what}`))
    rows.push({ fault, what, red, swallowed })
    if (red) continue
    failures.push(
      swallowed
        ? `${fault}: "${what}" was reported as a known gap rather than a failure, so it proves nothing`
        : `${fault}: "${what}" stayed green under the fault that breaks it. `
          + 'It is not a check. Fix it or delete it.',
    )
  }
  const caught = rows.filter((r) => r.fault === fault && r.red).length
  process.stderr.write(` ${caught}/${breaks.length} caught, suite exited ${status}\n`)
}

console.log(`\nmutations: ${cases.length} fault(s), ${rows.length} check(s) put to the test\n`)
for (const r of rows) {
  console.log(`  ${r.red ? 'red ' : 'GREEN'}  ${r.fault.padEnd(16)} ${r.what}`)
}

if (failures.length) {
  console.error(`\n${failures.length} failure(s):\n`)
  for (const f of failures) console.error(`  x ${f}`)
  process.exit(1)
}

console.log('\nevery check named here fails when the thing it guards is broken.')
