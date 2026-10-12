#!/usr/bin/env node
/**
 * THIRD_PARTY.md: what mcpd ships that other people wrote, and under which
 * licence.
 *
 * Two dependency trees reach a release. The Rust crate graph is linked into the
 * binary. The npm tree in web/ builds the interface that `include_dir!` then
 * compiles into that same binary. deny.toml polices the first and nothing
 * policed the second, so the licence question for the npm tree had no answer
 * on file. This script writes one answer for both, from the lock files, so it
 * is regenerated rather than remembered.
 *
 *     node tools/third-party.mjs          # rewrite THIRD_PARTY.md
 *     node tools/third-party.mjs --check  # fail if it is out of date
 *
 * The check runs in `make check` and in CI. A dependency bump that changes a
 * licence therefore changes a committed file in the same pull request, where a
 * reviewer sees it.
 */

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const ROOT = path.resolve(import.meta.dirname, '..')
const OUT = path.join(ROOT, 'THIRD_PARTY.md')
const CHECK = process.argv.includes('--check')

/* ------------------------------------------------------------ rust crates -- */

/**
 * Every crate reachable from a workspace member through a normal or a build
 * dependency. Dev-dependencies are left out on purpose: a test harness is not
 * distributed, and listing it would make this file claim more than the binary
 * carries.
 *
 * `--all-features` because the Postgres and ClickHouse backends are behind
 * Cargo features, and they bring the crates that reach a network. `--locked`
 * so this can never rewrite Cargo.lock as a side effect of writing a document.
 */
function rustCrates() {
  const raw = execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--all-features'],
    { cwd: ROOT, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  )
  const meta = JSON.parse(raw)
  const byId = new Map(meta.packages.map((p) => [p.id, p]))
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]))
  const members = new Set(meta.workspace_members)

  const seen = new Set()
  const queue = [...members]
  while (queue.length > 0) {
    const id = queue.pop()
    if (seen.has(id)) continue
    seen.add(id)
    const node = nodes.get(id)
    if (!node) continue
    for (const dep of node.deps) {
      // A dependency that is only ever a dev-dependency has every kind equal
      // to "dev". One normal or build edge anywhere is enough to ship it.
      const shipped = dep.dep_kinds.some((k) => k.kind !== 'dev')
      if (shipped && !seen.has(dep.pkg)) queue.push(dep.pkg)
    }
  }

  const crates = []
  for (const id of seen) {
    if (members.has(id)) continue
    const p = byId.get(id)
    crates.push({
      name: p.name,
      version: p.version,
      // `MIT/Apache-2.0` is the older spelling of `MIT OR Apache-2.0`, and the
      // two would otherwise be counted as different licences.
      license: p.license ? p.license.replace(/\s*\/\s*/g, ' OR ') : null,
      licenseFile: p.license_file,
      repository: p.repository,
    })
  }
  return crates
}

/* ----------------------------------------------------------- npm packages -- */

/**
 * Packages whose lock file entry carries no `license` field.
 *
 * npm writes the field from the package's own package.json, so an entry
 * without one is a package that declares no licence there. Each of these was
 * settled by reading the licence file the package ships, and the source of
 * that reading is recorded beside it. A new one fails the run rather than
 * being written up as "unknown", because "unknown" is the one answer this
 * document exists to remove.
 */
const NPM_UNDECLARED = {
  'zod-to-ts@1.2.0': {
    license: 'MIT',
    source: 'the LICENSE file in the package; its package.json has no license field',
  },
}

function npmPackages() {
  const lock = JSON.parse(readFileSync(path.join(ROOT, 'web/package-lock.json'), 'utf8'))
  const packages = []
  const undeclared = []
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (key === '') continue
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length)
    let license = entry.license ?? null
    let note = null
    if (!license) {
      const settled = NPM_UNDECLARED[`${name}@${entry.version}`]
      if (settled) {
        license = settled.license
        note = settled.source
      } else {
        undeclared.push(`${name}@${entry.version}`)
      }
    }
    packages.push({ name, version: entry.version, license, note, resolved: entry.resolved })
  }
  if (undeclared.length > 0) {
    console.error(
      `web/package-lock.json has ${undeclared.length} package(s) with no license field: `
      + `${undeclared.join(', ')}. Open each package's licence file under web/node_modules, `
      + 'add an entry to NPM_UNDECLARED in tools/third-party.mjs naming the licence and where '
      + 'you read it, then run `make third-party`.',
    )
    process.exit(1)
  }
  return packages
}

/* ---------------------------------------------------------------- the SDKs -- */

/**
 * The Kotlin SDK is the one SDK with third-party runtime dependencies. They are
 * declared in build.gradle.kts and Gradle writes no lock file, so they are read
 * from the build script here. The licence of each artefact was read from its
 * POM on Maven Central; a group not in this table fails the run so it gets
 * read too rather than guessed.
 */
const MAVEN_LICENSES = {
  'org.jetbrains.kotlinx': 'Apache-2.0',
}

function kotlinArtefacts() {
  const script = readFileSync(path.join(ROOT, 'sdks/kotlin/build.gradle.kts'), 'utf8')
  const artefacts = []
  for (const m of script.matchAll(/^\s*(api|implementation)\("([^:"]+):([^:"]+):([^"]+)"\)/gm)) {
    const [, , group, name, version] = m
    const license = MAVEN_LICENSES[group]
    if (!license) {
      console.error(
        `sdks/kotlin/build.gradle.kts declares ${group}:${name}:${version} and `
        + 'tools/third-party.mjs has no licence on file for that group. Read the licence from '
        + 'the artefact\'s POM on Maven Central and add the group to MAVEN_LICENSES.',
      )
      process.exit(1)
    }
    artefacts.push({ group, name, version, license })
  }
  return artefacts
}

/**
 * The other SDKs declare no runtime dependency, and the sentence saying so is
 * only worth printing while it is true. Each manifest is read; one that has
 * grown a dependency is named instead of being described as empty.
 */
function sdkManifests() {
  const ts = JSON.parse(readFileSync(path.join(ROOT, 'sdks/typescript/package.json'), 'utf8'))
  const py = readFileSync(path.join(ROOT, 'sdks/python/pyproject.toml'), 'utf8')
  const go = readFileSync(path.join(ROOT, 'sdks/go/go.mod'), 'utf8')
  const ex = readFileSync(path.join(ROOT, 'sdks/elixir/mix.exs'), 'utf8')
  return [
    { sdk: 'TypeScript', file: 'sdks/typescript/package.json', empty: Object.keys(ts.dependencies ?? {}).length === 0 },
    { sdk: 'Python', file: 'sdks/python/pyproject.toml', empty: /^dependencies\s*=\s*\[\s*\]/m.test(py) },
    { sdk: 'Go', file: 'sdks/go/go.mod', empty: !/^require\b/m.test(go) },
    { sdk: 'Elixir', file: 'sdks/elixir/mix.exs', empty: /defp deps, do: \[\]/.test(ex) },
  ]
}

/* --------------------------------------------------------------- the file -- */

function groupByLicence(items) {
  const groups = new Map()
  for (const item of items) {
    const key = item.license ?? 'No licence expression in the manifest'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key).push(item)
  }
  const sorted = [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))
  for (const [, list] of sorted) {
    list.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
  }
  return sorted
}

function render({ crates, npm, kotlin, manifests }) {
  const lines = []
  const w = (s = '') => lines.push(s)

  w('# Third-party software')
  w()
  w('<!-- GENERATED FILE. DO NOT EDIT. Written by tools/third-party.mjs from')
  w('     Cargo.lock and web/package-lock.json; run `make third-party`. -->')
  w()
  w('What mcpd ships that other people wrote, and under which licence. Two')
  w('dependency trees reach a release:')
  w()
  w('- **The Rust crate graph**, linked into the `mcpd` binary. `deny.toml` names')
  w('  the licences accepted there and CI enforces it with cargo-deny.')
  w('- **The npm tree in `web/`**, which builds the interface. The build output is')
  w('  static files, and `include_dir!` compiles those into the same binary. Some')
  w('  of these packages, such as React, end up bundled in that output; most are')
  w('  build tools that never leave the build machine. Both kinds are listed,')
  w('  because both reach the build.')
  w()
  w('Each list is written from the lock file, so it names the exact versions a')
  w('release is built from. Dev-dependencies of the Rust workspace are left out:')
  w('a test harness is not distributed.')
  w()
  w('## The copyleft packages in the web tree')
  w()
  w('Two families in the npm tree carry a copyleft licence, and neither is in the')
  w('`mcpd` binary or in anything it serves.')
  w()
  w('- **sharp** (`@img/sharp-libvips-*`, LGPL-3.0-or-later, and the `@img/sharp-*`')
  w('  platform packages whose expression combines Apache-2.0 with it). Astro')
  w('  depends on sharp to process images while the site is built.')
  w('- **lightningcss** (`lightningcss` and `lightningcss-*`, MPL-2.0). Tailwind')
  w('  depends on it to compile and minify CSS while the site is built.')
  w()
  w('Both are build-time tools. They run on the build machine, over this')
  w('project\'s own pages and stylesheets, and what they produce is this project\'s')
  w('own content. Neither library is linked into the binary, copied into the')
  w('output, or executed by the daemon. The LGPL and the MPL attach conditions to')
  w('distributing the licensed work, or a work that incorporates it; running the')
  w('work as a tool over your own files does not make the files it produces a')
  w('derivative of it. The project\'s determination is therefore that the')
  w('obligations of both licences do not extend to mcpd\'s release artefacts.')
  w('That is the project\'s reading, recorded here so the question has an answer')
  w('on file; it is not legal advice.')
  w()

  const crateGroups = groupByLicence(crates)
  w(`## Rust crates (${crates.length} crates, ${crateGroups.length} licence expressions)`)
  w()
  w('Reachable from a workspace member through a normal or build dependency,')
  w('with every Cargo feature on, on every target. A crate that is only used on')
  w('one platform is still listed.')
  w()
  for (const [license, list] of crateGroups) {
    w(`### ${license} (${list.length})`)
    w()
    for (const c of list) {
      const repo = c.repository ? ` (${c.repository})` : ''
      const file = c.license ? '' : `, licence file \`${c.licenseFile}\``
      w(`- ${c.name} ${c.version}${repo}${file}`)
    }
    w()
  }

  const npmGroups = groupByLicence(npm)
  w(`## npm packages in web/ (${npm.length} packages, ${npmGroups.length} licence expressions)`)
  w()
  w('Every entry in `web/package-lock.json`, including the platform-specific')
  w('binaries npm marks optional and the packages only the build needs.')
  w()
  for (const [license, list] of npmGroups) {
    w(`### ${license} (${list.length})`)
    w()
    for (const p of list) {
      const note = p.note ? ` (licence read from ${p.note})` : ''
      w(`- ${p.name} ${p.version}${note}`)
    }
    w()
  }

  w('## The SDKs')
  w()
  w('The Rust SDK, `crates/mcpd-sdk`, is a workspace member and its dependencies')
  w('are in the crate list above.')
  w()
  for (const m of manifests) {
    if (m.empty) {
      w(`- **${m.sdk}**: no third-party runtime dependency. \`${m.file}\` declares none.`)
    } else {
      w(`- **${m.sdk}**: \`${m.file}\` now declares dependencies; read it for the list.`)
    }
  }
  w('- **Kotlin**: the artefacts below, declared in `sdks/kotlin/build.gradle.kts`.')
  w('  Gradle writes no lock file, so their transitive dependencies are not')
  w('  enumerated here.')
  w()
  for (const a of kotlin) {
    w(`  - ${a.group}:${a.name} ${a.version}, ${a.license}`)
  }
  w()
  return `${lines.join('\n')}\n`
}

const text = render({
  crates: rustCrates(),
  npm: npmPackages(),
  kotlin: kotlinArtefacts(),
  manifests: sdkManifests(),
})

if (CHECK) {
  const current = existsSync(OUT) ? readFileSync(OUT, 'utf8') : ''
  if (current !== text) {
    console.error(
      'THIRD_PARTY.md is out of date with Cargo.lock, web/package-lock.json or '
      + 'sdks/kotlin/build.gradle.kts. Run `make third-party` and commit the result.',
    )
    process.exit(1)
  }
  console.log('THIRD_PARTY.md is current.')
} else {
  writeFileSync(OUT, text)
  console.log(`wrote ${path.relative(ROOT, OUT)}`)
}
