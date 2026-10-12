#!/usr/bin/env node
/**
 * The checks a compiler cannot make.
 *
 * Everything here exists because it broke once, or because it is the kind of
 * thing that rots silently: a sidebar entry pointing at a page nobody created,
 * a documented flag that was renamed a release ago, a code region the docs read
 * that somebody deleted.
 *
 * Run by `make check`, and therefore by CI.
 */

import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'

const ROOT = path.resolve(new URL('..', import.meta.url).pathname)
const problems = []
const notes = []
// A check that could not run. Printed apart from the passes, because a skip
// that prints as an `ok` is the kind of silent success this file exists to
// catch.
const notices = []

const fail = (where, what) => problems.push(`${where}: ${what}`)
const read = (p) => fs.readFileSync(path.join(ROOT, p), 'utf8')
const exists = (p) => fs.existsSync(path.join(ROOT, p))

/* ------------------------------------------- 1. docs nav resolves to pages */

function checkDocsNav() {
  const src = read('web/src/lib/docs.ts')
  const hrefs = [...src.matchAll(/href:\s*'([^']+)'/g)].map((m) => m[1])
  if (hrefs.length === 0) fail('web/src/lib/docs.ts', 'no entries found; did the format change?')

  for (const href of hrefs) {
    const rel = href.replace(/^\//, '')
    const candidates = [
      `web/src/pages/${rel}.astro`,
      `web/src/pages/${rel}/index.astro`,
      `web/src/pages/${rel || 'index'}.astro`,
    ]
    if (!candidates.some(exists)) {
      fail('docs nav', `"${href}" is in the sidebar but no page exists for it`)
    }
  }
  notes.push(`docs nav: ${hrefs.length} entries all resolve`)
}

/* ------------------------------ 2. every page carries a motion graphic */

const MOTION = [
  'StripChart', 'Waterfall', 'SignalFlow', 'PipelineDiagram',
  'SearchDemo', 'TerminalCast', 'Annunciator', 'RunCursor', 'LeaseProtocol',
  'SortingBench', 'LeaseBoard', 'RunDrum',
]

function walk(dir, out = []) {
  const full = path.join(ROOT, dir)
  if (!fs.existsSync(full)) return out
  for (const e of fs.readdirSync(full, { withFileTypes: true })) {
    const rel = path.join(dir, e.name)
    // An agent once ran npm install inside src/pages. Skip those rather than
    // reporting a thousand phantom pages.
    if (e.isDirectory()) {
      if (e.name === 'node_modules' || e.name === 'dist') continue
      walk(rel, out)
    } else if (e.name.endsWith('.astro')) out.push(rel)
  }
  return out
}

/** Like walk(), but for a chosen extension. */
function walkExt(dir, ext, out = []) {
  const full = path.join(ROOT, dir)
  if (!fs.existsSync(full)) return out
  for (const e of fs.readdirSync(full, { withFileTypes: true })) {
    const rel = path.join(dir, e.name)
    if (e.isDirectory()) {
      if (e.name === 'node_modules' || e.name === 'dist') continue
      walkExt(rel, ext, out)
    } else if (e.name.endsWith(ext)) out.push(rel)
  }
  return out
}

function checkMotion() {
  const pages = walk('web/src/pages')
  // The interface is itself live; the 404 page is a dead end, not a page to read.
  const exempt = new Set(['web/src/pages/app.astro', 'web/src/pages/404.astro'])
  let ok = 0
  for (const p of pages) {
    if (exempt.has(p)) continue
    const src = read(p)
    // An import is not a render. Require the component to actually appear as an
    // element, or a page that imports a graphic and never uses it passes.
    const rendered = MOTION.some((m) => new RegExp(`<${m}[\\s/>]`).test(src))
    if (rendered) ok += 1
    else if (MOTION.some((m) => src.includes(m))) {
      fail(p, 'imports a motion graphic but never renders it')
    } else {
      fail(p, 'no motion graphic; every page must carry at least one')
    }
  }
  notes.push(`motion: ${ok} pages carry a graphic`)
}

/* ------------------------------------ 3. example regions the docs read */

const REQUIRED_REGIONS = ['init', 'trace', 'score', 'query']
const EXAMPLES = {
  typescript: 'examples/typescript/quickstart.ts',
  python: 'examples/python/quickstart.py',
  rust: 'examples/rust/quickstart.rs',
  go: 'examples/go/quickstart.go',
  kotlin: 'examples/kotlin/Quickstart.kt',
  elixir: 'examples/elixir/quickstart.exs',
  curl: 'examples/curl/quickstart.sh',
}

/** The durable-run programs, read by the workflow, worker and run pages. */
const WORKFLOW_REGIONS = ['workflow', 'worker', 'run']
const WORKFLOW_EXAMPLES = {
  typescript: 'examples/typescript/workflow.ts',
  python: 'examples/python/workflow.py',
  rust: 'examples/rust/workflow.rs',
  go: 'examples/go/workflow.go',
  kotlin: 'examples/kotlin/Workflow.kt',
  elixir: 'examples/elixir/workflow.exs',
}

function checkExamples() {
  checkExampleSet(EXAMPLES, REQUIRED_REGIONS, 'LANGUAGES')
  checkExampleSet(WORKFLOW_EXAMPLES, WORKFLOW_REGIONS, 'the workflow pages')
  notes.push(`examples: ${Object.keys(EXAMPLES).length} quickstart and ${Object.keys(WORKFLOW_EXAMPLES).length} workflow programs present with all regions`)
}

function checkExampleSet(table, regions, listedIn) {
  for (const [lang, file] of Object.entries(table)) {
    if (!exists(file)) {
      fail(file, `missing: ${lang} is listed in ${listedIn} but has no example`)
      continue
    }
    const src = read(file)
    for (const region of regions) {
      if (!src.includes(`#region ${region}`)) {
        fail(file, `missing region "${region}"; the docs read it and the build will fail`)
      } else if (!src.includes(`#endregion ${region}`)) {
        fail(file, `region "${region}" is opened but never closed`)
      }
    }
    // A region that has grown into the whole file stops being an excerpt.
    for (const region of regions) {
      const lines = src.split('\n')
      const a = lines.findIndex((l) => l.includes(`#region ${region}`))
      const b = lines.findIndex((l) => l.includes(`#endregion ${region}`))
      if (a >= 0 && b > a) {
        const n = b - a - 1
        if (n > 46) fail(file, `region "${region}" is ${n} lines; keep an excerpt under 46`)
        // An excerpt that ends mid-block renders as code that does not compile,
        // which is worse than no example at all.
        const body = lines.slice(a + 1, b)
        const opens = body.join('').split('').filter((ch) => ch === '{' || ch === '(').length
        const closes = body.join('').split('').filter((ch) => ch === '}' || ch === ')').length
        if (opens !== closes) {
          fail(file, `region "${region}" is unbalanced (${opens} open, ${closes} close); the excerpt ends mid-block`)
        }
      }
    }
    // Long lines overflow the documentation column and get visually clipped.
    src.split('\n').forEach((l, i) => {
      if (l.length > 100) fail(`${file}:${i + 1}`, `line is ${l.length} chars; keep under 100`)
    })
  }
}

/* ------------------------------------------- the binary, and its settings */

// Three checks below read the built binary: the flags it has, the settings
// deploy/ can reach, and the lines `mcpd config` prints. With no binary they
// used to skip with a note, and the CI job that runs this file never built
// one, so none of the three comparisons AGENTS.md promises was ever made
// there. Under CI, or with MCPD_REQUIRE_BINARY set, a missing binary is a
// failure instead.
const REQUIRE_BINARY =
  Boolean(process.env.MCPD_REQUIRE_BINARY) || /^(1|true)$/i.test(process.env.CI ?? '')

/** The built daemon, or undefined. A failure under `check` when one is required. */
function builtBinary(check) {
  const bin = ['target/debug/mcpd', 'target/release/mcpd']
    .map((p) => path.join(ROOT, p))
    .find(fs.existsSync)
  if (!bin && REQUIRE_BINARY) {
    fail(
      check,
      'needs a built mcpd and there is none; run `cargo build -p mcpd` first. CI or ' +
        'MCPD_REQUIRE_BINARY is set, so this is a failure rather than a skip',
    )
  }
  return bin
}

/**
 * The serde field names of one Rust struct, read from its source.
 *
 * Read rather than listed here, so a key added to the config file cannot be
 * missed by a list nobody remembered to extend. `rename` is honoured and
 * `skip` leaves a field out, because those decide what the file accepts.
 */
function structFields(file, name) {
  const src = read(file)
  const at = src.search(new RegExp(`struct ${name}\\s*\\{`))
  if (at < 0) return null
  const open = src.indexOf('{', at)
  let depth = 0
  let end = open
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth += 1
    else if (src[i] === '}' && --depth === 0) {
      end = i
      break
    }
  }
  const fields = []
  let rename = null
  let skip = false
  for (const raw of src.slice(open + 1, end).split('\n')) {
    const line = raw.trim()
    if (line.startsWith('//')) continue
    if (line.startsWith('#[')) {
      const r = line.match(/rename\s*=\s*"([^"]+)"/)
      if (r) rename = r[1]
      if (/serde\((?:[^)]*,\s*)?skip(?:_deserializing)?\s*[,)]/.test(line)) skip = true
      continue
    }
    const m = line.match(/^(?:pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:/)
    if (m) {
      if (!skip) fields.push(rename ?? m[1])
      rename = null
      skip = false
    }
  }
  return fields
}

/**
 * Every struct the config file is read into, and where its keys sit in the
 * file. The top-level keys are `FileConfig`; the rest are blocks inside it.
 */
const CONFIG_FILE_STRUCTS = [
  ['crates/mcpd/src/main.rs', 'FileConfig', ''],
  ['crates/mcpd/src/main.rs', 'RunSettings', 'runs.'],
  ['crates/mcpd-server/src/state.rs', 'ProjectConfig', 'projects.<name>.'],
  ['crates/mcpd-server/src/derive.rs', 'DerivedMetrics', 'derived_metrics.'],
  ['crates/mcpd-server/src/aspects.rs', 'AspectSettings', 'aspects.'],
]

/* -------------------------------- 4b. every config file key is documented */

function checkConfigFileKeys() {
  const page = 'web/src/pages/docs/configuration.astro'
  if (!exists(page)) return
  const src = read(page)
  const missing = []
  let keys = 0
  for (const [file, name, prefix] of CONFIG_FILE_STRUCTS) {
    const fields = structFields(file, name)
    if (!fields?.length) {
      fail(file, `has no \`struct ${name}\` with fields, so its config file keys were checked against nothing`)
      continue
    }
    for (const f of fields) {
      keys += 1
      // Named in prose or in a JSON example. Either tells a reader the key
      // exists; neither means the only way to find it is the source.
      if (!src.includes(`<code>${f}</code>`) && !src.includes(`"${f}"`)) missing.push(prefix + f)
    }
  }
  if (missing.length) {
    fail(page, `never names these config file keys: ${missing.join(', ')}`)
    return
  }
  notes.push(`config file: ${keys} keys across ${CONFIG_FILE_STRUCTS.length} blocks, all documented`)
}

/* --------------------------- 4. documented flags exist in the binary */

function checkFlags() {
  const page = 'web/src/pages/docs/configuration.astro'
  if (!exists(page)) {
    notes.push('flags: configuration page not written yet, skipped')
    return
  }
  let help = ''
  try {
    // The binary's own help, and nothing else. The source used to stand in
    // when there was no binary, and clap derives most flag names from field
    // names, so `--listen` and nine others are not in the source as text and
    // were reported as documented flags the binary lacks.
    const bin = builtBinary('flags')
    if (!bin) {
      if (!REQUIRE_BINARY) notices.push('flags: no built binary, so no flag was compared')
      return
    }
    help = execFileSync(bin, ['--help'], { encoding: 'utf8' })
  } catch {
    if (REQUIRE_BINARY) fail('flags', 'mcpd --help did not run, so no flag was compared')
    else notes.push('flags: could not read mcpd --help, skipped')
    return
  }
  const documented = new Set(
    [...read(page).matchAll(/--([a-z][a-z0-9-]{2,})/g)].map((m) => m[1]),
  )
  const unknown = [...documented].filter((f) => !help.includes(`--${f}`))
  if (unknown.length) {
    fail(page, `documents flags the binary does not have: ${unknown.join(', ')}`)
  }

  // And the other direction, which is the one that actually rots: a flag added
  // to the binary and never written down. Only checked against real --help
  // output, because the source text also carries flag names in comments.
  const isHelp = help.includes('Usage:')
  if (isHelp) {
    const shipped = new Set(
      [...help.matchAll(/--([a-z][a-z0-9-]{2,})/g)]
        .map((m) => m[1])
        // clap's own, which mcpd does not own and should not document.
        .filter((f) => !['help', 'version'].includes(f)),
    )
    const undocumented = [...shipped].filter((f) => !documented.has(f))
    if (undocumented.length) {
      fail(page, `the binary has flags this page never mentions: ${undocumented.join(', ')}`)
    }
  }

  notes.push(
    `flags: ${documented.size} documented` + (isHelp ? ', and none missing' : ', all exist'),
  )
}

/* ------------------------------------------ 5. generated files untouched */

function checkGeneratedBanners() {
  const generated = [
    'crates/mcpd-core/src/generated.rs',
    'sdks/typescript/src/generated.ts',
    'web/src/generated/types.ts',
    'sdks/python/mcpd/_generated.py',
    'sdks/go/generated.go',
    'sdks/kotlin/src/main/kotlin/dev/mcpd/Generated.kt',
  ]
  for (const f of generated) {
    if (!exists(f)) { fail(f, 'generated file is missing; run `make gen`'); continue }
    if (!read(f).includes('GENERATED FILE')) {
      fail(f, 'lost its generated-file banner; it may have been hand-edited')
    }
  }
  notes.push(`generated: ${generated.length} files carry their banner`)
}

/* ------------------- 6. deployment manifests reference real settings */

function checkDeployEnv() {
  const files = []
  const walkAll = (dir) => {
    const full = path.join(ROOT, dir)
    if (!fs.existsSync(full)) return
    for (const e of fs.readdirSync(full, { withFileTypes: true })) {
      const rel = path.join(dir, e.name)
      if (e.isDirectory()) walkAll(rel)
      else if (/\.(ya?ml|env|example)$/.test(e.name) || e.name === 'Dockerfile') files.push(rel)
    }
  }
  walkAll('deploy')
  if (fs.existsSync(path.join(ROOT, 'Dockerfile'))) files.push('Dockerfile')
  if (!files.length) return

  let help = ''
  try {
    const bin = builtBinary('deploy env')
    help = bin ? execFileSync(bin, ['--help'], { encoding: 'utf8' }) : read('crates/mcpd/src/main.rs')
  } catch {
    if (REQUIRE_BINARY) fail('deploy env', 'mcpd --help did not run, so no setting was compared')
    else notes.push('deploy env: could not read mcpd --help, skipped')
    return
  }

  const seen = new Set()
  for (const f of files) {
    for (const m of read(f).matchAll(/MCPD_[A-Z0-9_]+/g)) {
      if (seen.has(m[0])) continue
      seen.add(m[0])
      if (!help.includes(m[0])) {
        fail(f, `sets "${m[0]}", which the daemon does not read`)
      }
    }
  }
  // And the other way round. Checking only that deploy/ names real settings
  // leaves a flag able to ship, be documented, pass every gate, and still be
  // impossible to set in the one deployment target this project ships a chart
  // for. Eight settings reached that state before this half existed.
  const exempt = new Set([
    // Helm carries structured settings in values.yaml, so a config file inside
    // the container would be a second source of truth for the same thing.
    'MCPD_CONFIG_FILE',
  ])
  const missing = []
  for (const m of help.matchAll(/\[env: (MCPD_[A-Z0-9_]+)/g)) {
    const name = m[1]
    if (exempt.has(name) || seen.has(name)) continue
    if (!missing.includes(name)) missing.push(name)
  }
  if (missing.length) {
    fail(
      'deploy/',
      `the daemon reads settings nothing in deploy/ can set: ${missing.join(', ')}. ` +
        'Add them to the chart, or name them in the exemption list in this file ' +
        'with the reason.'
    )
    return
  }
  // Only after both halves passed. A check that prints ok beside its own
  // failure is the exact thing this file exists to catch elsewhere.
  notes.push(
    `deploy env: ${seen.size} settings, all real, and every setting the daemon ` +
      `reads is reachable`
  )
}

/* ------------------ 6b. the documented `mcpd config` output is the real one */

/**
 * The `mcpd config` line that shows each flag and each top-level config file
 * key, by label.
 *
 * Comparing the page's labels with the binary's could not fail for a setting
 * that had no line at all, and eleven reached that state. A setting missing
 * from this table fails, so adding one means deciding where it is read back.
 */
const CONFIG_LINE_OF = {
  '--listen': 'listen',
  '--data': 'storage',
  '--clickhouse-url': 'storage',
  '--clickhouse-database': 'storage',
  '--clickhouse-user': 'storage',
  '--clickhouse-password': 'storage',
  '--postgres-url': 'storage',
  '--config-file': 'config file',
  '--log-format': 'log format',
  '--project': 'project',
  '--retention-days': 'retention',
  '--ingest-key': 'ingest auth',
  '--read-key': 'read auth',
  '--write-key': 'write auth',
  '--admin-key': 'admin auth',
  '--execute-key': 'execute auth',
  '--worker-key': 'worker auth',
  '--runs-data': 'runs',
  '--trusted-identity-header': 'identity',
  '--trusted-proxy-cidr': 'identity',
  '--client-ip-header': 'client ip',
  '--signup': 'signup',
  '--accounts-data': 'signup',
  '--sample': 'sampling',
  '--quota-per-minute': 'quota',
  '--no-redact': 'redaction',
  '--redact-key-contains': 'redaction',
  '--redact-max-string': 'redaction',
  '--redact-key': 'redact key',
  '--cors-origin': 'cors',
  '--request-timeout-seconds': 'timeout',
  '--max-body-bytes': 'limits',
  '--max-batch-signals': 'limits',
  '--forward-endpoint': 'forwarding',
  '--forward-header': 'forwarding',
  monitors: 'monitors',
  monitor_interval_seconds: 'monitors',
  slos: 'objectives',
  forward: 'forwarding',
  judge: 'judge',
  projects: 'projects',
  runs: 'run settings',
  max_body_bytes: 'limits',
  max_batch_signals: 'limits',
  derived_metrics: 'derived',
  aspects: 'aspects',
}

/** The translated page trees: every two-letter directory under the pages. */
function localeDirs() {
  return fs
    .readdirSync(path.join(ROOT, 'web/src/pages'), { withFileTypes: true })
    .filter((e) => e.isDirectory() && /^[a-z]{2}$/.test(e.name))
    .map((e) => `web/src/pages/${e.name}`)
}

function checkConfigOutput() {
  const page = 'web/src/pages/deploy.astro'
  const bin = builtBinary('config output')
  if (!bin || !exists(page)) {
    if (!REQUIRE_BINARY) notices.push('config output: no built binary, so no line was compared')
    return
  }

  // The page prints a pre-flight check. Showing an operator output the binary
  // no longer produces is worse than showing none, because the two lines they
  // are told to look for may not be there any more.
  let real
  try {
    real = execFileSync(
      bin,
      ['config', '--data', '/data/mcpd.db', '--project', 'default',
       '--retention-days', '30', '--ingest-key', 'k', '--read-key', 'k'],
      { encoding: 'utf8', env: { ...process.env, MCPD_LISTEN: '0.0.0.0:4317' } },
    )
  } catch {
    if (REQUIRE_BINARY) fail('config output', 'mcpd config did not run, so no line was compared')
    else notes.push('config output: mcpd config did not run, skipped')
    return
  }

  // Every cast of `mcpd config` on the deploy and install pages, in every
  // locale, from the command to the next command. Only the deploy page's
  // pre-flight block was compared, so the install page went on showing sixteen
  // of the lines after the binary printed twenty-nine, under a comment that
  // said those sixteen were the whole output. Anchored on the command rather
  // than on a title, because a title is translated and the command is not.
  const actual = real.trim().split('\n')
  // Only the labels are compared, not the values: a page shows one plausible
  // invocation, and pinning its numbers here would make the check about the
  // example rather than about the surface.
  const labelsOf = (lines) => lines.map((l) => l.split(/ {2,}/)[0].trim())
  const casts = []
  for (const dir of ['web/src/pages', ...localeDirs()]) {
    for (const rel of [`${dir}/deploy.astro`, `${dir}/docs/install.astro`]) {
      if (!exists(rel)) continue
      const src = read(rel)
      let at = src.indexOf("{ cmd: 'mcpd config'")
      while (at !== -1) {
        const rest = src.slice(at + 1)
        const stop = Math.min(
          ...[rest.indexOf('{ cmd:'), rest.indexOf(']}'), rest.indexOf(']')]
            .filter((i) => i !== -1),
        )
        const shown = [...rest.slice(0, stop).matchAll(/\{ out: '([^']+)'/g)].map((m) => m[1])
        casts.push([rel, shown])
        at = src.indexOf("{ cmd: 'mcpd config'", at + 1)
      }
    }
  }
  if (!casts.length) fail(page, 'no cast of mcpd config was found to compare')
  for (const [rel, shown] of casts) {
    const missing = labelsOf(actual).filter((l) => !labelsOf(shown).includes(l))
    const extra = labelsOf(shown).filter((l) => !labelsOf(actual).includes(l))
    if (missing.length) {
      fail(rel, `the mcpd config output is missing lines mcpd prints: ${missing.join(', ')}`)
    }
    if (extra.length) {
      fail(rel, `the mcpd config output shows lines mcpd no longer prints: ${extra.join(', ')}`)
    }
  }

  // Every setting has a line. The flags come from the binary's own help and
  // the keys from the struct the config file is read into, so neither list
  // can fall behind the daemon.
  let help = ''
  try {
    help = execFileSync(bin, ['--help'], { encoding: 'utf8' })
  } catch {
    fail('config output', 'mcpd --help did not run, so no flag was matched to a line')
  }
  const settings = [
    ...new Set(
      [...help.matchAll(/--([a-z][a-z0-9-]{2,})/g)]
        .map((m) => `--${m[1]}`)
        .filter((f) => !['--help', '--version'].includes(f)),
    ),
    ...(structFields('crates/mcpd/src/main.rs', 'FileConfig') ?? []),
  ]
  const printed = new Set(labelsOf(actual))
  const unmapped = settings.filter((s) => !(s in CONFIG_LINE_OF))
  const unprinted = settings.filter(
    (s) => s in CONFIG_LINE_OF && !printed.has(CONFIG_LINE_OF[s]),
  )
  if (unmapped.length) {
    fail(
      'crates/mcpd/src/main.rs',
      `mcpd config has no line for ${unmapped.join(', ')}; print one in config_lines and ` +
        'name it in CONFIG_LINE_OF in tools/check-consistency.mjs',
    )
  }
  if (unprinted.length) {
    fail(
      'crates/mcpd/src/main.rs',
      `mcpd config no longer prints the line for ${unprinted
        .map((s) => `${s} (${CONFIG_LINE_OF[s]})`)
        .join(', ')}`,
    )
  }
  notes.push(
    `config output: ${actual.length} lines, matching the binary, and every one of ` +
      `${settings.length} settings is read back by one`,
  )
}

/* ---------------------------------- 7. the non-text ink stays non-text */

function checkInkFour() {
  // ink-4 is light enough to fail WCAG AA on every surface in this system, so
  // it is a rule and tick colour only. Measured, not assumed: 2.2:1 on the page
  // ground. Using it for text puts unreadable words on the page.
  const files = [...walk('web/src/pages'), ...walk('web/src/components'), ...walk('web/src/layouts')]
    .concat(['web/src/components/app/Explorer.tsx'])
    .filter(exists)
  let hits = 0
  for (const f of files) {
    const src = read(f)
    src.split('\n').forEach((line, i) => {
      // `background-color` is a rule or tick, which is what ink-4 is for. The
      // lookbehind is what keeps this from matching the tail of that property
      // and reporting a 2px rule as unreadable text.
      if (/text-ink-4|(?<![\w-])color:\s*var\(--color-ink-4\)/.test(line)) {
        hits += 1
        fail(`${f}:${i + 1}`, 'uses ink-4 for text; it fails contrast on every surface. Use ink-3.')
      }
    })
  }
  if (!hits) notes.push('contrast: ink-4 is used for rules only, never text')
}

/* ------------------------- 7. SDKs agree on environment variable names */

const CANONICAL_ENV = new Set([
  'MCPD_ENDPOINT', 'MCPD_API_KEY', 'MCPD_PROJECT', 'MCPD_ENVIRONMENT',
  'MCPD_SERVICE_NAME', 'MCPD_SERVICE_VERSION', 'MCPD_SAMPLE', 'MCPD_ENABLED',
  'MCPD_BATCH_SIZE', 'MCPD_FLUSH_INTERVAL_MS', 'MCPD_MAX_QUEUE', 'MCPD_TIMEOUT_MS',
  'MCPD_CAPTURE_PAYLOADS', 'MCPD_LOG_LEVEL',
  // Read by tests and examples rather than by the client itself.
  'MCPD_TEST_ENDPOINT', 'MCPD_LOG',
  // Set by CI in any job that starts a daemon. It turns an environment-gated
  // suite whose server is absent into a failure instead of a vacuous pass.
  'MCPD_REQUIRE_LIVE',
])

function checkSdkEnvNames() {
  // Test directories are in here on purpose. The Rust SDK's live suite read
  // MCPD_LIVE_ENDPOINT, a name nothing ever set, so every test returned on its
  // first line and the suite reported three passes having contacted no server.
  // Scanning only `src` is what let a non-canonical name sit in a test file
  // for as long as it did.
  const roots = [
    'sdks/typescript/src', 'sdks/typescript/test',
    'sdks/python/mcpd', 'sdks/python/tests',
    'sdks/go',
    'sdks/kotlin/src/main/kotlin/dev/mcpd', 'sdks/kotlin/src/test/kotlin/dev/mcpd',
    'crates/mcpd-sdk/src', 'crates/mcpd-sdk/tests',
    'sdks/elixir/lib', 'sdks/elixir/test',
  ]
  const seen = new Map()
  for (const root of roots) {
    const dir = path.join(ROOT, root)
    if (!fs.existsSync(dir)) continue
    for (const f of fs.readdirSync(dir)) {
      const full = path.join(dir, f)
      if (!fs.statSync(full).isFile()) continue
      for (const m of fs.readFileSync(full, 'utf8').matchAll(/MCPD_[A-Z0-9_]+/g)) {
        if (!seen.has(m[0])) seen.set(m[0], `${root}/${f}`)
      }
    }
  }
  for (const [name, where] of seen) {
    if (!CANONICAL_ENV.has(name)) {
      fail(where, `reads "${name}", which is not in the canonical set; see sdks/SDK-CONTRACT.md`)
    }
  }
  notes.push(`sdk env: ${seen.size} variable names, all canonical`)

  // The other direction. A name in the contract's client table that one SDK
  // does not read is a value that stops working when the service is rewritten
  // in that language, which is the drift the table exists to prevent. The
  // scan covers each SDK's shipped sources, recursively.
  const clientRoots = {
    typescript: 'sdks/typescript/src',
    python: 'sdks/python/mcpd',
    go: 'sdks/go',
    kotlin: 'sdks/kotlin/src/main/kotlin/dev/mcpd',
    rust: 'crates/mcpd-sdk/src',
    elixir: 'sdks/elixir/lib',
  }
  const sources = (dir, out = []) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name)
      if (e.isDirectory()) {
        if (e.name !== 'node_modules' && e.name !== '_build' && e.name !== 'deps') sources(full, out)
      } else if (!e.name.endsWith('_test.go')) out.push(full)
    }
    return out
  }
  for (const [language, root] of Object.entries(clientRoots)) {
    const found = new Set()
    for (const f of sources(path.join(ROOT, root))) {
      for (const m of fs.readFileSync(f, 'utf8').matchAll(/MCPD_[A-Z0-9_]+/g)) found.add(m[0])
    }
    for (const name of CLIENT_ENV) {
      if (!found.has(name)) fail(root, `the ${language} client never reads ${name}; every SDK reads every client name in sdks/SDK-CONTRACT.md section 2`)
    }
  }
  notes.push(`sdk env: all six clients read the ${CLIENT_ENV.length} client names`)
}

/** The names the contract's section 2 table binds every client to read. */
const CLIENT_ENV = [
  'MCPD_ENDPOINT', 'MCPD_API_KEY', 'MCPD_PROJECT', 'MCPD_ENVIRONMENT',
  'MCPD_SERVICE_NAME', 'MCPD_SERVICE_VERSION', 'MCPD_SAMPLE', 'MCPD_ENABLED',
  'MCPD_BATCH_SIZE', 'MCPD_FLUSH_INTERVAL_MS', 'MCPD_MAX_QUEUE', 'MCPD_TIMEOUT_MS',
  'MCPD_CAPTURE_PAYLOADS', 'MCPD_LOG_LEVEL',
]

/* --------------------------------------------------- 6. prose discipline */

const BANNED = [
  'blazing', 'effortless', 'seamless', 'supercharge', 'game-chang',
  'revolutionary', 'cutting-edge', 'best-in-class', 'world-class',
  'easily', 'powerful', 'simply',
]

/**
 * Strip everything that is not prose a reader will see: Astro/TS frontmatter,
 * block and line comments, and code samples. Without this, a word used to
 * *explain* the rule in a source comment trips the rule.
 */
function proseOf(src, file) {
  let s = src
  if (file.endsWith('.astro')) s = s.replace(/^---\r?\n[\s\S]*?\r?\n---/, '')  // frontmatter
  s = s.replace(/```[\s\S]*?```/g, '')          // fenced code
  s = s.replace(/<Code[\s\S]*?\/>/g, '')        // <Code /> islands
  s = s.replace(/<pre[\s\S]*?<\/pre>/g, '')     // preformatted
  s = s.replace(/\{\/\*[\s\S]*?\*\/\}/g, '')   // {/* jsx comment */}
  s = s.replace(/<!--[\s\S]*?-->/g, '')         // html comment
  s = s.replace(/\/\*[\s\S]*?\*\//g, '')       // /* block */
  s = s.replace(/^\s*\/\/.*$/gm, '')           // // line
  s = s.replace(/`[^`\n]*`/g, '')               // inline code
  return s
}

function checkProse() {
  // AGENTS.md and this file define the banned list, so they necessarily
  // contain every word in it. Checking them would be a permanent false alarm.
  //
  // layouts/ and content/ are included deliberately: the page <title> is built
  // in a layout, and the generated reference pages are markdown in content/.
  // Both were invisible to this check once, and both shipped em-dashes.
  const files = [
    ...walk('web/src/pages'),
    ...walk('web/src/components'),
    ...walk('web/src/layouts'),
    ...walkExt('web/src/content', '.md'),
    'README.md', 'sdks/SDK-CONTRACT.md',
  ].filter(exists)

  let emdash = 0
  for (const f of files) {
    const prose = proseOf(read(f), f)
    for (const word of BANNED) {
      if (new RegExp(`\\b${word}`, 'i').test(prose)) fail(f, `contains banned word "${word}"`)
    }
    const n = (prose.match(/—/g) ?? []).length
    if (n > 0) { emdash += n; fail(f, `${n} em-dash character(s) in prose`) }
  }
  notes.push(`prose: checked ${files.length} files${emdash ? '' : ', no em-dashes'}`)
}

/* ------------------------------------------------- 8. the built pages are shareable */

/**
 * Everything from here to 8c reads a build. English alone is 32 pages; with the
 * eight translated locales it is closer to 290, and every rule below applies to
 * all of them. A locale exempted from these checks is a locale nobody reads.
 *
 * English stays at the root and the other eight sit under a prefix, so the same
 * document is `/docs/quickstart` and `/fr/docs/quickstart`. Two settings decide
 * what that looks like on disk. `build.format: 'file'` writes a page as
 * `docs/quickstart.html` rather than `docs/quickstart/index.html`, and folds a
 * folder index into its parent, so `src/pages/fr/index.astro` lands at
 * `fr.html` and not `fr/index.html`. `trailingSlash: 'never'` means the URL
 * that page is published under has neither the `.html` suffix nor a trailing
 * slash, and the home page is the bare origin. `routeOf` undoes both, so a
 * built file, a canonical link, an hreflang href and a sitemap entry all reduce
 * to one comparable string.
 */

/**
 * The nine locales, in the order they are shown. This constant is the fallback,
 * not the source of truth: `web/src/lib/locales.ts` holds the real table and
 * wins whenever it can be read, and this list only stands in if that file is
 * gone. Keeping it means the gate still knows what a locale prefix looks like,
 * so a build that grows a `/de/` tree nobody declared fails here rather than
 * being read as a document called "de".
 */
const FALLBACK_LOCALES = ['en', 'fr', 'es', 'zh', 'ja', 'ko', 'it', 'tr', 'pt']
const DEFAULT_LOCALE = 'en'

/** Looks like a language tag, so a stray top-level directory is not read as one. */
const LOCALE_SHAPED = /^[a-z]{2}(?:-[a-z0-9]{2,8})*$/i

/**
 * `--dist <path>` reads a build somewhere other than `web/dist`. It exists so
 * the locale rules can be proved against a tree that already has translated
 * pages, before any exist. Using it prints a notice: a gate that read a build
 * other than the one being shipped must never look like an ordinary run.
 */
function distDir() {
  const argv = process.argv.slice(2)
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith('--dist=')) return path.resolve(argv[i].slice(7))
    if (argv[i] === '--dist' && argv[i + 1]) return path.resolve(argv[i + 1])
  }
  return path.join(ROOT, 'web/dist')
}

const DIST = distDir()
const DIST_IS_DEFAULT = DIST === path.join(ROOT, 'web/dist')

/** The site origin, read from the config rather than repeated here. */
function siteOrigin() {
  const fromConfig = read('web/astro.config.mjs').match(/export const SITE\s*=\s*['"]([^'"]+)['"]/)
  if (fromConfig) return fromConfig[1].replace(/\/$/, '')
  const fromSite = read('web/src/lib/site.ts').match(/url:\s*['"]([^'"]+)['"]/)
  return (fromSite ? fromSite[1] : 'https://mcpd.sh').replace(/\/$/, '')
}

/** The text inside the brackets of the first list `re` matches, or null. */
function bracketedList(src, re, maxGap = 200) {
  const m = re.exec(src)
  if (!m) return null
  const from = m.index + m[0].length
  const open = src.indexOf('[', from)
  if (open < 0 || open - from > maxGap) return null
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '[') depth += 1
    else if (src[i] === ']') {
      depth -= 1
      if (depth === 0) return src.slice(open + 1, i)
    }
  }
  return null
}

/**
 * The URL segments out of a locale list, whichever shape it is written in:
 * plain strings, or objects naming the segment as `path`, `id` or `code`.
 * Taking every quoted string instead would pick up the labels a switcher shows
 * and the language tags beside them, and report those as locales of their own.
 */
function localeIds(region) {
  if (!region) return []
  const keyed = [...region.matchAll(/\b(?:path|id|code)\s*:\s*['"]([^'"]+)['"]/g)].map((m) => m[1])
  const ids = keyed.length ? keyed : [...region.matchAll(/['"]([^'"]+)['"]/g)].map((m) => m[1])
  return ids.filter((v) => LOCALE_SHAPED.test(v))
}

/**
 * The locale table, and the checks that the router serves exactly it.
 *
 * `web/src/lib/locales.ts` is the one list: `code` is the URL prefix and the
 * directory name, `tag` is the BCP 47 string published in `lang` and in
 * `hreflang`. The two differ where it matters (`/zh/` publishes `zh-Hans`), so
 * the tag is read here rather than assumed from the prefix.
 *
 * `astro.config.mjs` builds its `i18n` block from that same table, which is the
 * state this wants. If someone replaces it with a list written out by hand, the
 * two are compared and a difference fails: a router serving nine locales while
 * the table names eight is the kind of drift that surfaces as a dead link
 * months later.
 */
function resolveLocales() {
  const table = exists('web/src/lib/locales.ts') ? read('web/src/lib/locales.ts') : ''
  const region = bracketedList(table, /export\s+const\s+LOCALES\b[^=[]{0,80}=/)
  const ids = localeIds(region)
  const tags = new Map()
  for (const row of (region ?? '').matchAll(/\{[^}]*\}/g)) {
    const code = (row[0].match(/\bcode\s*:\s*['"]([^'"]+)['"]/) ?? [])[1]
    const tag = (row[0].match(/\btag\s*:\s*['"]([^'"]+)['"]/) ?? [])[1]
    if (code && tag) tags.set(code, tag)
  }

  if (ids.length === 0) {
    notices.push(
      `locales: no locale table found, so these checks use the built-in order [${FALLBACK_LOCALES.join(', ')}]`,
    )
    return { locales: FALLBACK_LOCALES, tags, from: 'the fallback list in tools/check-consistency.mjs' }
  }

  const declared = (table.match(/DEFAULT_LOCALE[^=\n]{0,40}=\s*['"]([^'"]+)['"]/) ?? [])[1]
  if (declared && declared !== DEFAULT_LOCALE) {
    fail(
      'web/src/lib/locales.ts',
      `declares DEFAULT_LOCALE "${declared}", but the URL scheme and these checks read "${DEFAULT_LOCALE}" ` +
        'as the locale served without a prefix',
    )
  }

  // The top-level block, matched by its own indentation. The sitemap
  // integration carries an `i18n` key of its own further down the same file,
  // and reading that one would check the wrong object and report a routing
  // block that is missing as present.
  const config = read('web/astro.config.mjs')
  const at = config.search(/^ {0,3}i18n\s*:/m)
  if (at < 0) {
    if (ids.length > 1) {
      fail(
        'web/astro.config.mjs',
        `has no top-level i18n block, so nothing routes the ${ids.length} locales in the table`,
      )
    }
  } else {
    const scope = config.slice(at, at + 600)
    const routed = localeIds(bracketedList(scope, /locales\s*:/))
    if (routed.length && (routed.length !== ids.length || !routed.every((id) => ids.includes(id)))) {
      fail(
        'web/astro.config.mjs',
        `routes [${routed.join(', ')}] but web/src/lib/locales.ts names [${ids.join(', ')}]`,
      )
    }
    const dl = (scope.match(/defaultLocale\s*:\s*['"]([^'"]+)['"]/) ?? [])[1]
    if (dl && dl !== DEFAULT_LOCALE) {
      fail('web/astro.config.mjs', `sets i18n.defaultLocale to "${dl}", but the table's default is "${DEFAULT_LOCALE}"`)
    }
    // The whole point of the scheme is that English keeps the URLs it already
    // publishes. Prefixing the default locale moves every one of them.
    if (/prefixDefaultLocale\s*:\s*true/.test(scope)) {
      fail(
        'web/astro.config.mjs',
        'sets routing.prefixDefaultLocale to true, which moves every English page under ' +
          `/${DEFAULT_LOCALE} and breaks every link already published`,
      )
    }
  }
  return { locales: ids, tags, from: 'web/src/lib/locales.ts' }
}

/**
 * Character ranges that occupy two Latin columns when rendered: the CJK blocks,
 * kana, hangul, the fullwidth forms and the emoji planes. Inclusive, sorted,
 * scanned rather than searched because the table is short.
 */
const WIDE_RANGES = [
  [0x1100, 0x115f], [0x2e80, 0x303e], [0x3041, 0x33ff], [0x3400, 0x4dbf],
  [0x4e00, 0x9fff], [0xa000, 0xa4cf], [0xa960, 0xa97f], [0xac00, 0xd7a3],
  [0xf900, 0xfaff], [0xfe10, 0xfe19], [0xfe30, 0xfe6f], [0xff00, 0xff60],
  [0xffe0, 0xffe6], [0x1f300, 0x1f64f], [0x1f900, 0x1f9ff], [0x20000, 0x3fffd],
]

/**
 * The width of a string in Latin columns.
 *
 * The 200 limit exists because a search engine cuts a description by pixel
 * width, near 160 Latin characters, and never by counting code points. A CJK
 * glyph fills two Latin columns, so counting characters gave zh, ja and ko
 * twice the width English gets before the same rule bit, and the check stopped
 * meaning what it says in the three locales where the limit is easiest to
 * overrun. Measured in columns one number means the same thing in all nine:
 * 200 Latin characters, or about 100 Chinese ones.
 *
 * Iterating the string yields code points, so an astral character counts once
 * rather than as the two UTF-16 units `String.length` would report.
 */
function columns(s) {
  if (!/[^\x00-\x7f]/.test(s)) return s.length
  let n = 0
  for (const ch of s) {
    const cp = ch.codePointAt(0)
    // A combining mark or a joiner renders inside the glyph beside it.
    if (cp === 0x200b || cp === 0x200d || (cp >= 0x0300 && cp <= 0x036f)) continue
    n += WIDE_RANGES.some(([lo, hi]) => cp >= lo && cp <= hi) ? 2 : 1
  }
  return n
}

const DESCRIPTION_COLUMNS = 200

/** A built file path, back into the URL it is published under. */
function routeOf(rel) {
  const cleaned = '/' + rel.split(path.sep).join('/').replace(/\.html$/, '')
  const route = cleaned.replace(/\/index$/, '')
  return route === '' ? '/' : route
}

/** One comparable form for a URL, whether it arrived with a trailing slash or not. */
function normUrl(u) {
  return (u ?? '').trim().replace(/\/+$/, '')
}

/**
 * Enough of the named entities to compare two renderings of one sentence.
 * Written as escapes so this file stays ASCII and no editor can quietly
 * normalise a punctuation mark the comparison depends on.
 */
const ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ',
  middot: '\u00b7', hellip: '\u2026', mdash: '\u2014', ndash: '\u2013',
  rsquo: '\u2019', lsquo: '\u2018', ldquo: '\u201c', rdquo: '\u201d',
}

function decodeEntities(s) {
  return s
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
    .replace(/&([a-z]+);/gi, (m, name) => ENTITIES[name.toLowerCase()] ?? m)
}

/** One run of visible text, flattened so two renderings of a sentence match. */
function normaliseRun(s) {
  return decodeEntities(s).replace(/\s+/g, ' ').trim().toLowerCase()
}

/**
 * The prose a reader sees, as a list of runs.
 *
 * Code is dropped on purpose. `mcpd trace list` reads the same in every
 * language and the documentation is full of it, so leaving code in would make
 * every translated page look half English. Script, style, template and svg go
 * with it: the instruments are drawn from data and their tick labels are not
 * prose.
 */
const NON_PROSE = ['script', 'style', 'svg', 'noscript', 'template', 'pre', 'code']

function visibleRuns(html) {
  const body = html.match(/<body[^>]*>([\s\S]*)<\/body>/i)
  let s = body ? body[1] : html
  s = s.replace(/<!--[\s\S]*?-->/g, '\n')
  for (const el of NON_PROSE) {
    s = s.replace(new RegExp(`<${el}\\b[^>]*>[\\s\\S]*?</${el}>`, 'gi'), '\n')
    s = s.replace(new RegExp(`<${el}\\b[^>]*/>`, 'gi'), '\n')
  }
  s = s.replace(/<[^>]+>/g, '\n')
  return s.split('\n').map(normaliseRun).filter(Boolean)
}

/**
 * A run long enough that two languages saying it identically is evidence rather
 * than coincidence. Product names, flags, numbers and menu labels sit far below
 * this; a sentence does not.
 */
const SUBSTANTIAL_COLUMNS = 40
const SHARED_RUN_LIMIT = 0.5

/**
 * Every built page, with the facts the checks below compare.
 *
 * The locale table arrives as an argument rather than being read here, because
 * the checks in `resolveLocales` are about the source and must run whether or
 * not anyone has built the site. Folding them in here meant a clean checkout
 * skipped them without saying so.
 */
function collectBuilt(table) {
  if (!DIST_IS_DEFAULT) notices.push(`built pages: reading ${DIST}, not web/dist`)
  if (!fs.existsSync(DIST)) return null

  const { locales, tags, from } = table
  const origin = siteOrigin()
  const files = []
  const walkDist = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name)
      if (e.isDirectory()) walkDist(full)
      else if (e.name.endsWith('.html')) files.push(full)
    }
  }
  walkDist(DIST)
  // An empty directory is not a build. Reporting "0 built pages, all correct"
  // is the silent success this file exists to catch, and it would let a run
  // that produced nothing at all read as a clean gate.
  if (files.length === 0) return null

  const pages = files.map((file) => {
    const rel = path.relative(DIST, file)
    const html = fs.readFileSync(file, 'utf8')
    const route = routeOf(rel)
    const first = route.split('/')[1] ?? ''
    const prefixed = locales.includes(first) && first !== DEFAULT_LOCALE
    const grab = (re) => (html.match(re) ?? [])[1]

    return {
      rel,
      // A prefix that reads as a language but is not one of ours. It is
      // reported once, by 8a, rather than again as a lang mismatch it explains.
      stray: !locales.includes(first) && first.length <= 3 && LOCALE_SHAPED.test(first),
      where: `${DIST_IS_DEFAULT ? 'web/dist' : DIST}/${rel}`,
      route,
      locale: prefixed ? first : DEFAULT_LOCALE,
      doc: prefixed ? route.slice(first.length + 1) || '/' : route,
      prefix: locales.includes(first) ? first : '',
      url: normUrl(origin + (route === '/' ? '' : route)),
      html,
      lang: grab(/<html[^>]*\blang="([^"]*)"/i),
      title: grab(/<title>([\s\S]*?)<\/title>/),
      description: grab(/<meta name="description" content="([^"]*)"/),
      canonical: normUrl(grab(/<link rel="canonical" href="([^"]*)"/)),
      noindex: /<meta name="robots" content="[^"]*noindex/i.test(html),
      alternates: headAlternates(html),
      runs: null,
    }
  })
  return { pages, locales, tags, origin, from }
}

/** `<link rel="alternate" hreflang>` in the head, in the order they appear. */
function headAlternates(html) {
  const out = []
  for (const tag of html.matchAll(/<link\b[^>]*>/gi)) {
    const t = tag[0]
    if (!/rel="alternate"/i.test(t)) continue
    const lang = (t.match(/hreflang="([^"]*)"/i) ?? [])[1]
    const href = (t.match(/href="([^"]*)"/i) ?? [])[1]
    if (lang) out.push({ lang, href: normUrl(href) })
  }
  return out
}

/** The alternates @astrojs/sitemap wrote, keyed by the URL they belong to. */
function sitemapAlternates() {
  const map = new Map()
  if (!fs.existsSync(DIST)) return map
  const files = fs
    .readdirSync(DIST)
    .filter((f) => /^sitemap-.+\.xml$/.test(f) && f !== 'sitemap-index.xml')
  for (const f of files) {
    const xml = fs.readFileSync(path.join(DIST, f), 'utf8')
    for (const block of xml.matchAll(/<url>([\s\S]*?)<\/url>/g)) {
      const loc = (block[1].match(/<loc>(.*?)<\/loc>/) ?? [])[1]
      if (!loc) continue
      const links = []
      for (const l of block[1].matchAll(/<xhtml:link\b([^>]*)>/g)) {
        const lang = (l[1].match(/hreflang="([^"]*)"/) ?? [])[1]
        const href = (l[1].match(/href="([^"]*)"/) ?? [])[1]
        if (lang && href) links.push({ lang, href: normUrl(href) })
      }
      map.set(normUrl(loc), links)
    }
  }
  return map
}

/** `zh` accepts `zh` and `zh-Hans`. It does not accept `zn`. */
const primaryOf = (tag) => String(tag ?? '').split('-')[0].toLowerCase()

function checkBuiltSeo(built) {
  if (!built) {
    notices.push(`seo: no built pages under ${DIST}, so nothing was checked. Run \`make ui\` first`)
    return
  }
  const { pages, tags } = built
  // Two pages in different languages may share a title. Two pages in the same
  // language may not, because that is the language a reader is searching in.
  const titles = new Map()
  const urls = new Map()
  let widest = 0

  for (const p of pages) {
    for (const [label, v] of [
      ['a title', p.title],
      ['a description', p.description],
      ['a canonical link', p.canonical],
      ['an og:image', (p.html.match(/<meta property="og:image" content="([^"]*)"/) ?? [])[1]],
      ['a twitter:card', (p.html.match(/<meta name="twitter:card" content="([^"]*)"/) ?? [])[1]],
    ]) {
      if (!v) fail(p.where, `has no ${label}`)
    }
    if (!p.html.includes('application/ld+json')) fail(p.where, 'has no structured data')

    if (p.description) {
      const w = columns(p.description)
      widest = Math.max(widest, w)
      if (w > DESCRIPTION_COLUMNS) {
        const chars = [...p.description].length
        fail(
          p.where,
          `description is ${w} columns wide (${chars} characters); keep it under ${DESCRIPTION_COLUMNS}. ` +
            'A CJK character counts as two, because a search engine cuts the snippet by width',
        )
      }
    }

    // The lang attribute is what a screen reader switches voice on and what a
    // search engine believes over the URL, so a French page announcing English
    // is worse than a page with no annotation at all.
    const tag = tags.get(p.locale)
    if (!p.lang) fail(p.where, 'has no lang attribute on <html>')
    else if (p.stray) {
      // 8a already reports the prefix; a lang mismatch here would only repeat it.
    } else if (tag && p.lang.toLowerCase() !== tag.toLowerCase()) {
      fail(
        p.where,
        `is served at ${p.route}, where web/src/lib/locales.ts publishes lang="${tag}", but says lang="${p.lang}"`,
      )
    } else if (!tag && primaryOf(p.lang) !== p.locale) {
      fail(p.where, `is served at ${p.route}, which declares the ${p.locale} locale, but says lang="${p.lang}"`)
    }

    if (p.canonical && p.canonical !== p.url) {
      fail(p.where, `canonical points at ${p.canonical}, but the page is published at ${p.url}`)
    }

    if (urls.has(p.url)) fail(p.where, `publishes the same URL as ${urls.get(p.url)}: ${p.url}`)
    else urls.set(p.url, p.rel)

    const h1s = (p.html.match(/<h1/g) ?? []).length
    if (h1s !== 1) fail(p.where, `has ${h1s} h1 elements; a page needs exactly one`)

    if (p.title) {
      const key = `${p.locale} ${p.title}`
      if (titles.has(key)) fail(p.where, `shares its title with ${titles.get(key)}, and both are ${p.locale}`)
      else titles.set(key, p.rel)
    }
  }

  // A translated page whose title or description is byte-identical to the
  // English page's is untranslated in the two strings a reader meets before
  // the page even opens: the browser tab and the search result. Uniqueness
  // within a locale does not catch it, because one English title is unique
  // inside its own locale. The Chinese home page shipped with the English
  // tagline in its tab for exactly this reason, while the other seven
  // locales had translated theirs.
  // Some titles are meant to be identical: a product name (TypeScript,
  // OpenTelemetry) is not translated in any language, and a loanword a
  // language genuinely uses (French "Configuration", Portuguese "Workflows")
  // is the correct translation of itself. That decision is already recorded
  // per locale in web/src/lib/docs-strings.ts, so this reads it rather than
  // guessing from the string.
  const intentional = new Map()
  const navSource = read('web/src/lib/docs-strings.ts')
  for (const [, route, body] of navSource.matchAll(/'(\/[^']*)': \{([^}]*)\}/g)) {
    for (const [, code, value] of body.matchAll(/(\w\w): ['"]((?:[^'"\\]|\\.)*)['"]/g)) {
      intentional.set(`${code} ${route}`, value)
    }
  }

  const english = new Map()
  for (const p of pages) {
    if (p.locale === DEFAULT_LOCALE) english.set(p.doc, p)
  }
  let compared = 0
  let identical = 0
  for (const p of pages) {
    if (p.locale === DEFAULT_LOCALE) continue
    const en = english.get(p.doc)
    if (!en) continue
    compared++
    if (p.title && en.title && p.title === en.title) {
      // The declared navigation title for this locale and document. When it
      // matches what the page publishes, the page is translated and its
      // translation is the same word.
      const declared = intentional.get(`${p.locale} ${p.doc}`)
      const leading = p.title.split(' \u00b7 ')[0]
      if (declared && declared === leading) identical++
      else {
        fail(p.where, `carries the English title "${p.title}" word for word, and web/src/lib/docs-strings.ts does not declare that as the ${p.locale} title; the tab and the search result are the first text a reader sees`)
      }
    }
    if (p.description && en.description && p.description === en.description) {
      fail(p.where, `carries the English description word for word`)
    }
  }
  notes.push(
    `seo: ${pages.length} built pages, titles unique within each locale, ${compared} translated pages checked against their English original (${identical} share a title their locale declares), widest description ${widest} of ${DESCRIPTION_COLUMNS} columns`,
  )
}

/* ------------------------------------- 8a. every locale is whole, or visibly not */

function checkLocaleCoverage(built) {
  if (!built) return
  const { pages, locales, from } = built
  const counted = new Map(locales.map((l) => [l, 0]))
  // Coverage counts the pages a search engine may index. The operator UI is
  // noindex and English by design, so counting it would leave every locale one
  // page short for ever and the report would be one nobody could act on.
  const docsPerLocale = new Map(locales.map((l) => [l, new Set()]))

  for (const p of pages) {
    // routing.prefixDefaultLocale is false, so English is served from the root.
    // A page under /en as well would publish every English document twice.
    if (p.prefix === DEFAULT_LOCALE) {
      fail(
        p.where,
        `is under /${DEFAULT_LOCALE}, but ${DEFAULT_LOCALE} is the default locale and is served from the root`,
      )
    }
    if (p.stray) {
      const first = p.route.split('/')[1]
      fail(p.where, `sits under /${first}, which reads as a locale prefix but is not in the locale list (${from})`)
    }
    counted.set(p.locale, (counted.get(p.locale) ?? 0) + 1)
    if (!p.noindex) docsPerLocale.get(p.locale)?.add(p.doc)
  }

  const total = docsPerLocale.get(DEFAULT_LOCALE)?.size ?? 0
  const line = locales.map((l) => `${l} ${counted.get(l) || 'none'}`).join(', ')
  notes.push(`locales: ${line} built pages (list from ${from})`)

  // A language stuck at a third of the pages is a fact somebody needs before a
  // release, not after. It is reported rather than failed: a translation lands
  // page by page, and a gate that failed on the first page would stop the work.
  for (const l of locales) {
    if (l === DEFAULT_LOCALE || (counted.get(l) ?? 0) === 0) continue
    const have = docsPerLocale.get(l)?.size ?? 0
    if (have < total) {
      const missing = [...(docsPerLocale.get(DEFAULT_LOCALE) ?? [])].filter((d) => !docsPerLocale.get(l)?.has(d))
      notices.push(
        `locales: ${l} covers ${have} of the ${total} indexable documents. ` +
          `Still missing: ${missing.slice(0, 6).join(', ')}` +
          (missing.length > 6 ? `, and ${missing.length - 6} more` : ''),
      )
    }
  }
}

/* ------------------------------- 8b. the locales point at each other, both ways */

/**
 * Every locale's copy of a document must name every other copy, and one page in
 * the set must be the x-default. Half an annotation is worse than none: a page
 * naming two of nine locales tells a search engine the other seven are separate
 * documents that happen to look alike, which is how a translated site ends up
 * competing with itself.
 *
 * Two sources are accepted, because two are produced. The head carries
 * `<link rel="alternate" hreflang>`; @astrojs/sitemap writes `<xhtml:link>` into
 * the sitemap for every URL it can group by path. Whichever is used has to be
 * complete. That integration emits no x-default at all, so a document annotated
 * only there still has to declare one in the head.
 */
function checkHreflang(built) {
  if (!built) return
  const { pages, locales, tags } = built
  const byUrl = new Map(pages.map((p) => [p.url, p]))
  const byDoc = new Map()
  for (const p of pages) {
    if (!byDoc.has(p.doc)) byDoc.set(p.doc, [])
    byDoc.get(p.doc).push(p)
  }
  const sitemap = sitemapAlternates()
  const malformed = new Set()

  // Shape first, on every page. A document with one locale needs no alternates,
  // but a page that has written some still has to have written them truthfully.
  for (const p of pages) {
    if (!shapeOfAlternates(p, p.alternates, byUrl, locales, tags, 'the head')) malformed.add(p.url)
  }
  for (const [url, links] of sitemap) {
    const p = byUrl.get(url)
    if (!p) {
      fail(`${DIST_IS_DEFAULT ? 'web/dist' : DIST} sitemap`, `lists ${url}, which is not a built page`)
      continue
    }
    if (links.length && !shapeOfAlternates(p, links, byUrl, locales, tags, 'the sitemap')) malformed.add(p.url)
  }

  const multi = [...byDoc.entries()].filter(([, ps]) => ps.length > 1)
  if (multi.length === 0) {
    const only = [...new Set(pages.map((p) => p.locale))].join(', ')
    notices.push(
      `hreflang: ${byDoc.size} documents, every one of them ${only} only, so no page has an alternate to declare yet`,
    )
    return
  }

  let complete = 0
  for (const [doc, ps] of multi) {
    const want = ps.map((p) => p.locale).sort()
    const headUsed = ps.some((p) => p.alternates.length > 0)
    const mapUsed = ps.some((p) => (sitemap.get(p.url) ?? []).length > 0)

    if (!headUsed && !mapUsed) {
      fail(
        `document ${doc}`,
        `is built in ${want.length} locales (${want.join(', ')}) but no copy declares an hreflang alternate, ` +
          'in the head or in the sitemap',
      )
      continue
    }
    let ok = ps.every((p) => !malformed.has(p.url))
    if (headUsed) ok = coversEveryLocale(doc, ps, want, (p) => p.alternates, 'the head') && ok
    if (mapUsed) ok = coversEveryLocale(doc, ps, want, (p) => sitemap.get(p.url) ?? [], 'the sitemap') && ok

    // Exactly one x-default for the document, and it names the page a reader
    // with no matching language should land on, which is the default locale.
    const defaults = new Set()
    for (const p of ps) {
      const here = [...p.alternates, ...(sitemap.get(p.url) ?? [])].filter(
        (a) => a.lang.toLowerCase() === 'x-default',
      )
      const targets = new Set(here.map((a) => a.href))
      if (targets.size > 1) {
        fail(p.where, `declares hreflang="x-default" ${here.length} times, pointing at ${targets.size} different pages`)
        ok = false
      }
      for (const d of targets) defaults.add(d)
    }
    if (defaults.size === 0) {
      fail(`document ${doc}`, `is built in ${want.length} locales but nothing declares hreflang="x-default"`)
      ok = false
    } else if (defaults.size > 1) {
      fail(`document ${doc}`, `x-default resolves to ${defaults.size} different pages: ${[...defaults].join(', ')}`)
      ok = false
    } else {
      const target = [...defaults][0]
      const home = ps.find((p) => p.locale === DEFAULT_LOCALE)
      if (home && target !== home.url) {
        fail(`document ${doc}`, `x-default points at ${target}, but the ${DEFAULT_LOCALE} copy is ${home.url}`)
        ok = false
      }
      if (!home) notices.push(`hreflang: ${doc} has no ${DEFAULT_LOCALE} copy, so its x-default points at ${target}`)
    }
    if (ok) complete += 1
  }
  if (complete) {
    notes.push(`hreflang: ${complete} of ${multi.length} multi-locale documents name every sibling and one x-default`)
  }
}

/** Each alternate names a locale that exists, and reaches the page it claims. */
function shapeOfAlternates(page, links, byUrl, locales, tags, source) {
  const seen = new Set()
  let ok = true
  const bad = (what) => { fail(page.where, what); ok = false }
  for (const a of links) {
    const lang = a.lang.toLowerCase()
    if (seen.has(lang)) {
      bad(`declares hreflang="${a.lang}" twice in ${source}`)
      continue
    }
    seen.add(lang)
    if (lang !== 'x-default' && !locales.includes(primaryOf(lang))) {
      bad(`declares hreflang="${a.lang}" in ${source}, which is not one of the site's locales`)
      continue
    }
    // `zh` and `zh-Hans` are both well formed, and publishing one in the head
    // and the other in the sitemap tells a search engine two different things
    // about the same page. The table decides which string is published.
    const want = tags.get(primaryOf(lang))
    if (lang !== 'x-default' && want && lang !== want.toLowerCase()) {
      bad(
        `declares hreflang="${a.lang}" in ${source}, but web/src/lib/locales.ts publishes ` +
          `${primaryOf(lang)} as "${want}"`,
      )
      continue
    }
    if (!a.href) {
      bad(`declares hreflang="${a.lang}" in ${source} with no href`)
      continue
    }
    const target = byUrl.get(a.href)
    if (!target) {
      bad(`points hreflang="${a.lang}" at ${a.href} in ${source}, which is not a built page`)
      continue
    }
    if (target.doc !== page.doc) {
      bad(
        `points hreflang="${a.lang}" at ${a.href} in ${source}, which is a different document ` +
          `(${target.doc}, not ${page.doc})`,
      )
    }
    if (lang !== 'x-default' && target.locale !== primaryOf(lang)) {
      bad(`points hreflang="${a.lang}" at ${a.href} in ${source}, which is the ${target.locale} page`)
    }
  }
  return ok
}

/** Every copy of a document names every locale of it, including itself. */
function coversEveryLocale(doc, ps, want, linksOf, source) {
  let ok = true
  for (const p of ps) {
    const named = new Set(
      linksOf(p)
        .filter((a) => a.lang.toLowerCase() !== 'x-default')
        .map((a) => primaryOf(a.lang)),
    )
    const missing = want.filter((l) => !named.has(l))
    if (missing.length) {
      fail(
        p.where,
        `names ${named.size} of the ${want.length} locales of ${doc} in ${source}; missing ${missing.join(', ')}`,
      )
      ok = false
    }
  }
  return ok
}

/* ------------------------------ 8c. a translated page is actually translated */

/**
 * A page under a locale prefix still carrying the English text is the failure
 * mode of a half-finished translation: it builds, it validates, it links
 * correctly, and it is wrong in the only way that matters.
 *
 * The test is byte identity against the English copy of the same document,
 * measured twice. First the whole visible body: identical prose is not a
 * translation. Then sentence by sentence, over the runs at least 40 columns
 * wide, which is long enough that two languages producing the same string is
 * evidence rather than coincidence. Anything shorter is a menu label, a flag, a
 * number or a product name, and those match honestly. Code blocks are removed
 * before any of it, because `mcpd trace list` reads the same in every language.
 *
 * False positives: a page whose long prose is mostly untranslatable strings, a
 * URL or a licence name written out in full, could cross the shared fraction on
 * its own. The 40-column floor, the four-run minimum and the 50 per cent
 * threshold make that unlikely rather than impossible, so the failure prints
 * the shared sentences. Whoever reads it can tell in a few seconds whether the
 * page is untranslated or the measure misfired.
 */
function checkTranslated(built) {
  if (!built) return
  const { pages } = built
  const english = new Map()
  for (const p of pages) if (p.locale === DEFAULT_LOCALE) english.set(p.doc, p)

  const runsOf = (p) => (p.runs ??= visibleRuns(p.html))
  const substantial = (runs) => runs.filter((r) => columns(r) >= SUBSTANTIAL_COLUMNS)

  const translated = pages.filter((p) => p.locale !== DEFAULT_LOCALE)
  if (translated.length === 0) {
    notices.push(`translation: ${DEFAULT_LOCALE} is the only locale built, so there is no translated page to compare`)
    return
  }

  let compared = 0
  for (const p of translated) {
    const en = english.get(p.doc)
    if (!en) continue
    compared += 1

    if (
      p.description &&
      p.description === en.description &&
      columns(p.description) >= SUBSTANTIAL_COLUMNS
    ) {
      fail(p.where, `has the same meta description as the ${DEFAULT_LOCALE} page, word for word`)
    }

    const mine = runsOf(p)
    const theirs = runsOf(en)
    if (mine.join(' ') === theirs.join(' ')) {
      fail(p.where, `has the same visible text as ${en.where}; it is served as ${p.locale} but was never translated`)
      continue
    }

    const mineBig = substantial(mine)
    const theirsBig = new Set(substantial(theirs))
    if (mineBig.length < 4) continue
    const shared = mineBig.filter((r) => theirsBig.has(r))
    const ratio = shared.length / mineBig.length
    if (ratio >= SHARED_RUN_LIMIT) {
      const sample = shared.slice(0, 3).map((r) => `"${r.slice(0, 60)}${r.length > 60 ? '...' : ''}"`)
      const verdict =
        ratio === 1
          ? 'every sentence on it is still the English one'
          : `${Math.round(ratio * 100)} per cent of it is still English`
      fail(
        p.where,
        `keeps ${shared.length} of its ${mineBig.length} sentences word for word from ${en.where}, so ` +
          `${verdict}. For example ${sample.join(', ')}`,
      )
    }
  }
  if (compared) notes.push(`translation: ${compared} translated pages differ from their ${DEFAULT_LOCALE} originals`)
}

/* ----------------- 8d. the enforced scope is the documented scope */

/**
 * Compare each endpoint's `auth` value in the spec against the `Scope` the
 * handler behind its route actually passes to `authorize`.
 *
 * This exists because the two drifted, and drifting was a security bug rather
 * than a documentation one: the reference said `DELETE /v1/api/traces/{id}`
 * needed `admin` while the handler enforced `Scope::Read`, so a key advertised
 * as read-only could destroy a run. Nothing in the build compared a documented
 * scope with an enforced one, so nothing caught it. This is that comparison.
 *
 * The route table is the join, not the endpoint id: an id and a function name
 * are allowed to differ, and going through `lib.rs` also means an endpoint that
 * is specified, documented and implemented but never routed fails here rather
 * than shipping as a 404. Read from source on purpose, so the check works in a
 * clean checkout with nothing built.
 */
/**
 * The scopes the spec may name and the `Scope` variants the handlers may pass,
 * as one table. A scope in the spec this table lacks, or a variant in a handler
 * it lacks, fails by name rather than as a silent mismatch.
 */
const SCOPES = new Set(['ingest', 'read', 'write', 'admin', 'execute', 'worker'])

function checkEndpointScopes() {
  const spec = JSON.parse(read('spec/mcpd.spec.json'))
  const router = 'crates/mcpd-server/src/lib.rs'
  const handlerFiles = [
    'crates/mcpd-server/src/query.rs',
    'crates/mcpd-server/src/ingest.rs',
    'crates/mcpd-server/src/runs.rs',
    'crates/mcpd-server/src/accounts.rs',
    'crates/mcpd-server/src/grpc.rs',
  ]
  if (!exists(router)) {
    fail(router, 'is missing; the scope check cannot run without the route table')
    return
  }
  // A missing handler file is named and the check still runs over the rest, so
  // one absent file does not hide a wrong scope in another.
  const present = handlerFiles.filter((f) => {
    if (exists(f)) return true
    fail(f, 'is missing; the endpoints it should serve will show as unrouted or unhandled below')
    return false
  })
  const routerSrc = read(router)

  // Every handler body, from its `pub async fn` to the next one. Only the
  // authorisation call inside it is read later, never the whole body, so a
  // scope named in a comment cannot change the answer.
  const bodies = new Map()
  for (const file of present) {
    const src = read(file)
    const starts = [...src.matchAll(/^pub async fn (\w+)\s*\(/gm)]
    starts.forEach((m, j) => {
      const end = j + 1 < starts.length ? starts[j + 1].index : src.length
      bodies.set(m[1], { file, body: src.slice(m.index, end) })
    })
  }

  // Keyed by the SHAPE of the path, with every parameter reduced to `{}`.
  // Axum names a path parameter `:id` where the spec names it `{dataset_id}`;
  // that difference is a wording choice, not a routing one, and failing the
  // build over it would be noise in a check whose whole value is that a failure
  // means something. The names are reported below as a note instead.
  const shape = (p) => p.replace(/[:{](\w+)}?/g, '{}')
  // Routes are read from lib.rs and from every handler file, because a module
  // may assemble its own `Router` and hand it to lib.rs to merge.
  const routes = new Map()
  const routePaths = new Map()
  for (const src of [routerSrc, ...present.map(read)]) {
    for (const m of src.matchAll(
      /\.route\(\s*"([^"]+)"\s*,\s*(get|post|put|patch|delete)\(\s*(?:\w+::)?(\w+)\s*\)/g,
    )) {
      const key = `${m[2].toUpperCase()} ${shape(m[1])}`
      routes.set(key, m[3])
      routePaths.set(key, m[1])
    }
  }

  let checked = 0
  for (const ep of spec.endpoints) {
    if (ep.auth === 'none') continue
    const where = `${ep.method} ${ep.path}`
    // An account session is not a scope: no API key passes it, so the handler
    // must call `authorize_session` and nothing that takes a key.
    if (ep.auth === 'session') {
      const fn = routes.get(`${ep.method} ${shape(ep.path)}`)
      const found = fn && bodies.get(fn)
      if (!found) {
        fail('crates/mcpd-server/src/lib.rs', `${where} is in the spec but no route reaches a handler for it`)
      } else if (!/authorize_session\(/.test(found.body) || /authorize(?:_project)?\(/.test(found.body)) {
        fail(
          `${found.file}: ${fn}`,
          `serves ${where}, documented \`session\`, and must call \`authorize_session\` and no key check`,
        )
      } else {
        checked += 1
      }
      continue
    }
    if (!SCOPES.has(ep.auth)) {
      fail('spec/mcpd.spec.json', `${where} names scope \`${ep.auth}\`, which is not one of ${[...SCOPES].join(', ')}`)
      continue
    }
    const fn = routes.get(`${ep.method} ${shape(ep.path)}`)
    if (!fn) {
      fail('crates/mcpd-server/src/lib.rs', `${where} is in the spec but no route reaches it`)
      continue
    }
    const found = bodies.get(fn)
    if (!found) {
      fail('endpoint scopes', `${where} routes to ${fn}, which is not a \`pub async fn\` in ${present.join(', ')}`)
      continue
    }
    // Only these two spellings count. A handler that authorises through a
    // helper of its own is invisible here and fails as authorising nothing,
    // which is the safe direction: spell the call out, or teach this check.
    const scopes = [...found.body.matchAll(/authorize(?:_project)?\([^;]*?Scope::(\w+)/gs)].map(
      (m) => m[1].toLowerCase(),
    )
    if (scopes.length === 0) {
      fail(
        `${found.file}: ${fn}`,
        `serves ${where}, documented \`${ep.auth}\`, and authorises nothing that this check ` +
          'recognises: it looks for `authorize(..., Scope::X)` or `authorize_project(..., Scope::X)` in the handler body',
      )
      continue
    }
    const unknown = scopes.find((sc) => !SCOPES.has(sc))
    if (unknown) {
      fail(`${found.file}: ${fn}`, `passes Scope::${unknown}, which is not a scope this check knows; add it to SCOPES with its spec value`)
      continue
    }
    const wrong = scopes.find((sc) => sc !== ep.auth)
    if (wrong) {
      fail(
        `${found.file}: ${fn}`,
        `serves ${where}: the spec says \`${ep.auth}\` and the handler enforces \`${wrong}\`. ` +
          'Change one of them; the spec is the source of truth.',
      )
      continue
    }
    checked += 1
  }

  // The other direction is a note rather than a failure. A route may be
  // deliberately outside the specified surface while it settles, but leaving it
  // unnamed is how it becomes the normal state.
  const specified = new Set(spec.endpoints.map((e) => `${e.method} ${shape(e.path)}`))
  const unspecified = [...routes.keys()].filter((k) => !specified.has(k))
  const renamed = [
    ...new Set(
      spec.endpoints
        .map((e) => [e, routePaths.get(`${e.method} ${shape(e.path)}`)])
        .filter(([e, routed]) => routed && routed.replace(/:(\w+)/g, '{$1}') !== e.path)
        .map(([e, routed]) => `${e.path} is routed as ${routed}`),
    ),
  ]
  notes.push(
    `endpoint scopes: ${checked} routes enforce the scope the spec documents` +
      (unspecified.length ? `; ${unspecified.length} routed but unspecified: ${unspecified.join(', ')}` : '') +
      (renamed.length ? `; ${renamed.length} with a differently named parameter: ${renamed.join(', ')}` : ''),
  )
}

/* ---------------- 8e. the definition schema names every construct kind */

/**
 * `spec/definition.schema.json` validates a posted definition, and every node
 * in it carries a `kind`. The set of kinds the schema accepts has to be the set
 * the daemon's cursor can name, which the spec holds as `ConstructKind`, minus
 * `end`, which the cursor reaches and no document declares. A kind in one and
 * not the other is a node that validates and cannot run, or runs and cannot be
 * written.
 */
function checkConstructKinds() {
  const file = 'spec/definition.schema.json'
  if (!exists(file)) {
    notices.push(`${file} is absent, so the construct-kind check did not run; it is written by the definition schema track`)
    return
  }
  const kinds = new Set()
  const walk = (node) => {
    if (Array.isArray(node)) { node.forEach(walk); return }
    if (!node || typeof node !== 'object') return
    const kind = node.properties?.kind
    if (kind && typeof kind === 'object') {
      if (typeof kind.const === 'string') kinds.add(kind.const)
      for (const v of kind.enum ?? []) if (typeof v === 'string') kinds.add(v)
    }
    for (const v of Object.values(node)) walk(v)
  }
  walk(JSON.parse(read(file)))
  const spec = JSON.parse(read('spec/mcpd.spec.json'))
  const expected = new Set(spec.enums.ConstructKind.values.map((v) => v.name).filter((n) => n !== 'end'))
  const missing = [...expected].filter((k) => !kinds.has(k))
  const extra = [...kinds].filter((k) => !expected.has(k))
  if (missing.length) fail(file, `accepts no node of kind ${missing.join(', ')}; ConstructKind in the spec has them`)
  if (extra.length) fail(file, `accepts node kinds the spec's ConstructKind does not have: ${extra.join(', ')}`)
  if (!missing.length && !extra.length) notes.push(`definition schema: ${kinds.size} node kinds match ConstructKind`)
}

/* ------------------- 8g. the sidebar has a translation for every entry */

/**
 * A page added to the documentation map, and never given a sidebar title.
 *
 * `docs-strings.ts` carries two tables: `TITLES`, keyed by route, and
 * `LEGENDS`, keyed by the English section name. `docTitle` and `docLegend`
 * fall back to English when a row is missing, which is the right behaviour at
 * runtime and the wrong behaviour in review: the sidebar renders, nothing
 * fails, and eight locales quietly show an English word.
 *
 * Both files already export a function for this. `missingDocStrings` in
 * `docs-strings.ts` says in its own doc comment that it is "read by the
 * consistency gate", and `missingTranslations` in `ui-strings.ts` says a gate
 * can call it. Neither had a caller, so both were documentation of a check
 * that did not exist. This is that check. It parses the tables rather than
 * importing them, because Node cannot import TypeScript, which is the same
 * approach `checkBuiltSeo` already takes over the same file.
 */
function checkSidebarStrings() {
  const codes = (resolveLocales().locales ?? []).filter((c) => c && c !== 'en')
  if (!codes.length) {
    notices.push('sidebar strings: no locales resolved, so coverage was not checked')
    return
  }
  const map = read('web/src/lib/docs.ts')
  const strings = read('web/src/lib/docs-strings.ts')

  const routes = [...map.matchAll(/href:\s*'([^']+)'/g)].map((m) => m[1])
  const legends = [...new Set([...map.matchAll(/legend:\s*'([^']+)'/g)].map((m) => m[1]))]

  /* A row is `'/docs/x': { fr: '…', … }` for a title and `Start: { … }` for a
     legend, so the key is quoted in one table and bare in the other. */
  const rowsOf = (re) => {
    const out = new Map()
    for (const m of strings.matchAll(re)) {
      const body = m[2]
      const have = new Set([...body.matchAll(/(\w\w):\s*['"]/g)].map((x) => x[1]))
      out.set(m[1], have)
    }
    return out
  }
  const titles = rowsOf(/'(\/[^']*)':\s*\{([^}]*)\}/g)
  const sections = rowsOf(/^\s{2}([A-Z][A-Za-z ]*):\s*\{([^}]*)\}/gm)

  let checked = 0
  const before = problems.length
  const short = (where, have) => codes.filter((c) => !have.has(c)).map((c) => `${where} has no ${c}`)

  for (const r of routes) {
    const have = titles.get(r)
    if (!have) fail('docs-strings.ts', `no sidebar title for ${r}, so eight locales show the English one`)
    else {
      for (const m of short(`title ${r}`, have)) fail('docs-strings.ts', m)
      checked += 1
    }
  }
  for (const l of legends) {
    const have = sections.get(l)
    if (!have) fail('docs-strings.ts', `no sidebar legend for the ${l} section, so eight locales show the English word`)
    else {
      for (const m of short(`legend ${l}`, have)) fail('docs-strings.ts', m)
      checked += 1
    }
  }
  if (checked === routes.length + legends.length && problems.length === before) {
    notes.push(`sidebar: ${routes.length} titles and ${legends.length} legends, each in all ${codes.length} translated locales`)
  }

  /* The interface catalogue marks an untranslated string with a sentinel
     rather than leaving it out, so counting it is cheap and does not need a
     build. Reported rather than failed: a partly translated catalogue is a
     known state here, and the locale coverage check is reported the same way. */
  const ui = read('web/src/lib/ui-strings.ts')
  const sentinel = (ui.match(/const\s+UNTRANSLATED\s*=\s*['"]([^'"]*)['"]/) ?? [])[1]
  if (sentinel) {
    const waiting = (ui.match(new RegExp(`:\\s*${sentinel.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`, 'g')) ?? []).length
    if (waiting) notices.push(`ui strings: ${waiting} still carry the untranslated sentinel`)
  }
}

/* ------------------- 8h. every published watch is one the daemon implements */

/**
 * Compare the `MonitorWatch` values the spec publishes against the `Watch`
 * variants the daemon actually reads, and against the table the alerting page
 * shows an operator.
 *
 * This exists because the three drifted and nothing noticed. `run_repetition`
 * was added to `spec/mcpd.spec.json`, generated into the Rust model, all six
 * SDK types, the OpenAPI document and the published reference, and the daemon
 * had no variant for it. An operator who configured the documented value got a
 * daemon that refused to read its configuration at all, which is the worst
 * shape a documentation defect can take: the reference was right, the product
 * was wrong, and the only symptom was a process that would not start.
 *
 * Read from source rather than from a build, so it works in a clean checkout.
 * The daemon's enum carries `#[serde(rename_all = "snake_case")]`, so the wire
 * spelling is the variant name in snake case and the comparison is exact.
 */
function checkMonitorWatches() {
  const impl = 'crates/mcpd-server/src/monitor.rs'
  const page = 'web/src/pages/docs/alerting.astro'
  const spec = JSON.parse(read('spec/mcpd.spec.json'))
  const published = spec.enums?.MonitorWatch?.values?.map((v) => v.name)
  if (!published?.length) return fail('spec/mcpd.spec.json', 'has no MonitorWatch enum to check watches against')

  if (!exists(impl)) return fail(impl, 'is absent, so the published watches were compared against nothing')
  const body = read(impl).match(/pub enum Watch \{([\s\S]*?)^\}/m)
  if (!body) return fail(impl, 'has no `pub enum Watch` block; did the monitor module change shape?')
  const snake = (v) => v.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase()
  const implemented = new Set(
    [...body[1].matchAll(/^\s{4}([A-Z][A-Za-z0-9]*),$/gm)].map((m) => snake(m[1])),
  )

  const missing = published.filter((w) => !implemented.has(w))
  const extra = [...implemented].filter((w) => !published.includes(w))
  if (missing.length) {
    fail(impl, `the spec publishes ${missing.join(', ')} and this enum has no variant for it, so a monitor written from the reference will not parse`)
  }
  if (extra.length) {
    fail(impl, `reads watches the spec never published: ${extra.join(', ')}`)
  }

  // And the table an operator actually reads. AGENTS.md: a table that is
  // missing a variant is a table nobody can trust.
  let undocumented = []
  if (exists(page)) {
    const shown = read(page)
    undocumented = published.filter((w) => !shown.includes(`<code>${w}</code>`))
    if (undocumented.length) fail(page, `its watch table never names ${undocumented.join(', ')}`)
  } else {
    notices.push(`${page} is absent, so the watch table was not checked`)
  }

  if (!missing.length && !extra.length && !undocumented.length) {
    notes.push(`monitors: ${published.length} watches, published and implemented and documented alike`)
  }
}

/* ------------------------------- 8f. one licence, and every package carries it */

/**
 * The licence claim has to be true in the tree, not only in the README.
 *
 * Three ways it rots. A package is published to a registry that reads the
 * licence from the package directory, and the directory has no licence file, so
 * the artefact ships undeclared however loudly the repository says Apache-2.0.
 * The copyright holder is named in prose but left as the stock placeholder in
 * the licence itself. Or a second, narrower licence appears over part of the
 * source, which is how a project that calls itself open source stops being it.
 *
 * The reference product this one is measured against does exactly that: it
 * keeps its role model, its authorisation checks and its trace intelligence
 * under a separate licence that forbids production use without a written
 * agreement. mcpd's answer is that there is one licence and no held-back
 * directory, so the answer is checked here rather than promised in a document.
 */
function checkLicensing() {
  const HOLDER = 'Internet Technology Services LLC'

  if (!exists('LICENSE')) return fail('LICENSE', 'is missing')
  const root = read('LICENSE')
  if (root.includes('[name of copyright owner]')) {
    fail('LICENSE', 'still carries the stock placeholder instead of a copyright holder')
  } else if (!root.includes(HOLDER)) {
    fail('LICENSE', `does not name ${HOLDER}`)
  }
  if (!exists('NOTICE')) fail('NOTICE', 'is missing, and Apache-2.0 section 4(d) redistributes it')
  else if (!read('NOTICE').includes(HOLDER)) fail('NOTICE', `does not name ${HOLDER}`)

  /* Each SDK is installed on its own from a registry, so each one is its own
     redistribution and needs the text beside it. The manifest field is the
     declaration; the file is the licence. A package needs both. */
  const packages = [
    { dir: 'sdks/typescript', manifest: 'package.json', declares: /"license":\s*"Apache-2\.0"/ },
    { dir: 'sdks/python', manifest: 'pyproject.toml', declares: /license\s*=\s*\{\s*text\s*=\s*"Apache-2\.0"/ },
    // Go has no licence field. The file in the module root is the whole
    // declaration, which is why its absence was invisible.
    { dir: 'sdks/go', manifest: 'go.mod', declares: null },
    { dir: 'sdks/kotlin', manifest: 'build.gradle.kts', declares: /Apache License, Version 2\.0/ },
    { dir: 'sdks/elixir', manifest: 'mix.exs', declares: /licenses:\s*\["Apache-2\.0"\]/ },
    { dir: 'crates/mcpd-sdk', manifest: 'Cargo.toml', declares: /license\.workspace\s*=\s*true/ },
  ]
  let carried = 0
  for (const p of packages) {
    const where = `${p.dir}/LICENSE`
    if (!exists(where)) {
      fail(where, 'is missing, so the published artefact ships without its licence text')
    } else if (read(where) !== root) {
      fail(where, 'differs from the root LICENSE, so two packages state different terms')
    } else {
      carried += 1
    }
    const manifest = `${p.dir}/${p.manifest}`
    if (!exists(manifest)) fail(manifest, 'is missing')
    else if (p.declares && !p.declares.test(read(manifest))) {
      fail(manifest, 'does not declare Apache-2.0')
    }
  }
  /* The packaging step decides what actually reaches the tarball, and both of
     these default to an allow-list that does not include the licence. */
  if (exists('sdks/typescript/package.json')) {
    const files = JSON.parse(read('sdks/typescript/package.json')).files ?? []
    if (!files.includes('LICENSE')) fail('sdks/typescript/package.json', '`files` omits LICENSE, so npm packs without it')
  }
  if (exists('sdks/python/pyproject.toml') && !/include\s*=\s*\[[^\]]*"LICENSE"/.test(read('sdks/python/pyproject.toml'))) {
    fail('sdks/python/pyproject.toml', '`include` omits LICENSE, so the wheel ships without it')
  }
  if (carried === packages.length) {
    notes.push(`licensing: all ${carried} published packages carry the same Apache-2.0 text, held by ${HOLDER}`)
  }

  /* One licence, everywhere. A directory named `ee`, `enterprise` or `pro`
     carrying its own licence file is the shape this project has said it will
     not have, so it is refused mechanically rather than by memory. */
  const held = []
  const scan = (dir) => {
    for (const e of fs.readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
      if (!e.isDirectory()) continue
      if (['node_modules', 'target', '.git', 'dist', '_build', 'deps', '.astro'].includes(e.name)) continue
      const here = `${dir}/${e.name}`
      if (/^(ee|enterprise|pro|commercial)$/i.test(e.name)) held.push(here)
      scan(here)
    }
  }
  for (const top of ['crates', 'sdks', 'web/src', 'spec', 'tools']) if (exists(top)) scan(top)
  for (const h of held) fail(h, 'is a licence-holdback directory, and mcpd ships one licence for everything')

  const strays = []
  const walkLicences = (dir) => {
    for (const e of fs.readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
      if (e.isDirectory()) {
        if (['node_modules', 'target', '.git', 'dist', '_build', 'deps', '.astro'].includes(e.name)) continue
        // A directory holding its own `.git` is another checkout, such as a
        // git worktree, and its files are that checkout's, not this tree's.
        if (fs.existsSync(path.join(ROOT, dir, e.name, '.git'))) continue
        walkLicences(`${dir}/${e.name}`)
      } else if (/^LICEN[CS]E/i.test(e.name)) {
        const rel = `${dir}/${e.name}`.replace(/^\.\//, '')
        if (rel !== 'LICENSE' && !packages.some((p) => rel === `${p.dir}/LICENSE`)) strays.push(rel)
      }
    }
  }
  walkLicences('.')
  for (const s of strays) fail(s, 'is a licence file this check does not know about, so the terms are unreviewed')
  if (!held.length && !strays.length) notes.push('licensing: one licence covers the tree, with no held-back directory')
}

/* ------------------------------------------------------- 9. spec is sound */

function checkSpec() {
  const spec = JSON.parse(read('spec/mcpd.spec.json'))
  let undocumented = 0
  for (const [name, def] of Object.entries({ ...spec.types, ...spec.entities })) {
    for (const f of def.fields) if (!f.doc) { undocumented += 1; fail(`spec ${name}.${f.name}`, 'has no doc string') }
  }
  for (const e of Object.values(spec.enums)) {
    for (const v of e.values) if (!v.doc) { undocumented += 1; fail(`spec enum ${v.name}`, 'has no doc string') }
  }
  if (!undocumented) notes.push('spec: every field and enum value is documented')
}

/* -------------------- 10. every hardcoded version is the workspace version */

// The release tags the workspace version in Cargo.toml and checks nothing
// else, so a version typed anywhere else drifts silently: a chart that pulls
// last release's image, an SDK that reports one version in its manifest and
// another in its user agent. Every one of them is held to Cargo.toml here,
// and so is every image and chart reference, to the repository the chart's
// values name, which is also what release.yml checks the push target against.
function checkVersions() {
  const before = problems.length
  const ws = read('Cargo.toml').match(/\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/m)
  if (!ws) {
    fail('Cargo.toml', 'has no version under [workspace.package], so there is nothing to hold the others to')
    return
  }
  const want = ws[1]
  let checked = 0
  const hold = (file, what, re) => {
    if (!exists(file)) {
      fail(file, `is gone, but checkVersions in this file still expects ${what} in it; update the list`)
      return
    }
    const m = read(file).match(re)
    if (!m) {
      fail(file, `no ${what} found where checkVersions expects one; update the pattern in this file`)
      return
    }
    checked += 1
    if (m[1] !== want) {
      fail(file, `${what} is ${m[1]}, but the workspace version in Cargo.toml is ${want}. Set it to ${want}: the versions move together, or one release ships pieces that disagree`)
    }
  }

  hold('deploy/helm/mcpd/Chart.yaml', 'the chart version', /^version:\s*"?([^"\s]+)"?\s*$/m)
  hold('deploy/helm/mcpd/Chart.yaml', 'the chart appVersion', /^appVersion:\s*"?([^"\s]+)"?\s*$/m)
  hold('sdks/typescript/package.json', 'the package version', /^\s*"version":\s*"([^"]+)"/m)
  hold('sdks/typescript/src/client.ts', 'the user agent version', /'mcpd-sdk-typescript\/([^']+)'/)
  hold('sdks/python/pyproject.toml', 'the project version', /^version\s*=\s*"([^"]+)"/m)
  hold('sdks/python/mcpd/__init__.py', '__version__', /^__version__\s*=\s*"([^"]+)"/m)
  hold('sdks/python/mcpd/client.py', 'the user agent version', /"mcpd-python\/([^"]+)"/)
  hold('sdks/go/transport.go', 'Version, which the user agent carries', /^\s*Version\s*=\s*"([^"]+)"/m)
  hold('sdks/kotlin/build.gradle.kts', 'the project version', /^version\s*=\s*"([^"]+)"/m)
  hold('sdks/kotlin/src/main/kotlin/dev/mcpd/Client.kt', 'the user agent version', /"mcpd-kotlin\/([^"]+)"/)
  hold('examples/kotlin/build.gradle.kts', 'the SDK version it depends on', /"dev\.mcpd:mcpd-kotlin:([^"]+)"/)
  hold('sdks/elixir/mix.exs', 'the project version', /^\s*version:\s*"([^"]+)"/m)
  hold('sdks/elixir/lib/mcpd/http.ex', 'the user agent version', /"mcpd-elixir\/([^"]+)"/)

  // Image and chart references, in the manifests and on every page. Only a
  // reference with a version in it is held to the version; `latest` and an
  // untagged name are left alone, and the pages say why they use them.
  const values = read('deploy/helm/mcpd/values.yaml').match(/^image:\s*\n\s+repository:\s*(\S+)/m)
  if (!values) {
    fail('deploy/helm/mcpd/values.yaml', 'has no image.repository, which every image reference is held to')
    return
  }
  const image = values[1]
  const owner = image.split('/')[1]
  const files = []
  const collect = (dir, re) => {
    const full = path.join(ROOT, dir)
    if (!fs.existsSync(full)) return
    for (const e of fs.readdirSync(full, { withFileTypes: true })) {
      const rel = path.join(dir, e.name)
      if (e.isDirectory()) collect(rel, re)
      else if (re.test(e.name)) files.push(rel)
    }
  }
  collect('deploy', /\.(ya?ml|example|md)$/)
  collect('web/src/pages', /\.astro$/)
  let refs = 0
  for (const f of files) {
    const text = read(f)
    // `v?` so a tag written with a leading v is caught: the release pushes
    // `0.1.0`, never `v0.1.0`, so that reference would not pull.
    for (const m of text.matchAll(/ghcr\.io\/([a-z0-9._-]+)\/mcpd(?![\w/-])(?::(v?[0-9][\w.+-]*))?/g)) {
      refs += 1
      if (`ghcr.io/${m[1]}/mcpd` !== image) {
        fail(f, `names the image ghcr.io/${m[1]}/mcpd, but image.repository in deploy/helm/mcpd/values.yaml is ${image}, which is where the release pushes`)
      } else if (m[2] && m[2] !== want) {
        fail(f, `pins ${image}:${m[2]}, but the workspace version in Cargo.toml is ${want}. Set the tag to ${want}`)
      }
    }
    for (const m of text.matchAll(/ghcr\.io\/([a-z0-9._-]+)\/charts\/mcpd/g)) {
      refs += 1
      if (m[1] !== owner) {
        fail(f, `names the chart ghcr.io/${m[1]}/charts/mcpd, but the release pushes it under ghcr.io/${owner}/charts, beside ${image}`)
      }
    }
  }

  // The release's binaries and the image are built by the same compiler, so
  // an archive and the image of one tag are the same build. The Dockerfile's
  // rust tag names a minor version; release.yml pins a release of it.
  const docker = read('Dockerfile').match(/FROM\s+(?:--platform=\S+\s+)?rust:(\d+\.\d+)(?:\.\d+)?-/)
  const release = read('.github/workflows/release.yml').match(/toolchain:\s*['"]?(\d+\.\d+(?:\.\d+)?)['"]?/)
  if (!docker || !release) {
    fail('Dockerfile', 'the rust image tag or the toolchain in release.yml could not be read; update the patterns in checkVersions')
  } else if (release[1] !== docker[1] && !release[1].startsWith(`${docker[1]}.`)) {
    fail('.github/workflows/release.yml', `builds the binaries with Rust ${release[1]}, but the Dockerfile builds the image with rust:${docker[1]}. Pin both to the same release`)
  }

  if (problems.length === before) {
    notes.push(`versions: ${checked} versions and ${refs} image and chart references agree with ${want} and ${image}`)
  }
}

/* ------------------------------------------------------------------ run */

checkDocsNav()
checkMotion()
checkExamples()
checkFlags()
checkConfigFileKeys()
checkDeployEnv()
checkVersions()
checkConfigOutput()
checkInkFour()
checkSdkEnvNames()
checkGeneratedBanners()
checkEndpointScopes()
checkConstructKinds()
checkMonitorWatches()
checkLicensing()
checkSidebarStrings()
checkProse()
const built = collectBuilt(resolveLocales())
checkBuiltSeo(built)
checkLocaleCoverage(built)
checkHreflang(built)
checkTranslated(built)
checkSpec()

for (const n of notes) console.log(`  ok  ${n}`)
for (const n of notices) console.log(`  --  ${n}`)

if (problems.length) {
  console.error(`\n${problems.length} problem${problems.length === 1 ? '' : 's'}:\n`)
  for (const p of problems) console.error(`  ✗  ${p}`)
  console.error('\nSee AGENTS.md for what to change alongside what.')
  process.exit(1)
}
console.log('\nconsistency: all checks passed')
