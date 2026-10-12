/**
 * What the cross-language conformance suite currently proves.
 *
 * Read from `tests/e2e/run.mjs` at build time, never written down twice. The
 * panel that draws the suite and the landing page that makes a claim about it
 * both read this, so a language added to the harness cannot leave a headline
 * saying "seven" beside a panel showing eight.
 *
 * This is the same rule `snippets.ts` follows for code samples, and the same
 * one AGENTS.md states for every graphic: a drawing is computed from the thing
 * it depicts, or it drifts away from it and nothing catches it.
 */

import fs from 'node:fs'
import path from 'node:path'

/** The harness, relative to the repository root. Named once. */
export const HARNESS = 'tests/e2e/run.mjs'

/**
 * Where the channel list lives, which is no longer where the checks live.
 *
 * The harness grew a `languages.mjs` when the suite was extended, and this
 * reader kept looking for the list inside `run.mjs`. The site build FAILED
 * rather than drawing a panel with zero channels, which is exactly what this
 * module is for: better a build that stops than a landing page still stating a
 * number the suite no longer produces.
 */
export const CHANNEL_SOURCE = 'tests/e2e/languages.mjs'

/** How a language id in the harness is spelled on the page. */
const LABELS: Record<string, string> = {
  typescript: 'TypeScript',
  python: 'Python',
  rust: 'Rust',
  go: 'Go',
  kotlin: 'Kotlin',
  elixir: 'Elixir',
  curl: 'cURL',
}

const read = (rel: string) => fs.readFileSync(path.resolve(process.cwd(), '..', rel), 'utf8')

const source = read(HARNESS)
const channelSource = read(CHANNEL_SOURCE)

/** The channels, in the order the harness runs them. */
export const CHANNELS: string[] = [
  ...channelSource.matchAll(/^\s*id: '([a-z]+)',$/gm),
].map((m) => {
  const label = LABELS[m[1]]
  if (!label) {
    throw new Error(
      `${HARNESS} runs a language this site cannot name: "${m[1]}". ` +
        'Add it to LABELS in web/src/lib/conformance.ts.',
    )
  }
  return label
})

/**
 * How many invariants the harness asserts for each language.
 *
 * Counted inside the loop over the languages. The checks after that loop
 * compare the languages with each other, so they are not per-channel and are
 * not multiplied by the channel count.
 */
const loop = source.slice(source.indexOf('for (const lang of LANGUAGES) {'))
export const PER_CHANNEL = [...loop.matchAll(/\bcheck\((?:id|lang\.id),/g)].length

if (CHANNELS.length === 0 || PER_CHANNEL === 0) {
  throw new Error(
    `the conformance harness no longer parses: ${CHANNELS.length} channels in ` +
      `${CHANNEL_SOURCE} and ${PER_CHANNEL} per-channel checks in ${HARNESS}. ` +
      'The site reads those files, so it cannot state a number the suite stopped ' +
      'producing. Fix the reader in web/src/lib/conformance.ts.',
  )
}

/** Every cell the panel draws. */
export const TOTAL_CHECKS = CHANNELS.length * PER_CHANNEL

const WORDS = [
  'no', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine',
  'Ten', 'Eleven', 'Twelve',
]

/** A small count spelled out, because prose does not open with a digit. */
export function spell(n: number): string {
  return WORDS[n] ?? String(n)
}
