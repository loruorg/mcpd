/**
 * Code samples, read from the real example programs.
 *
 * Documentation never contains a copy of a code sample. It names a file and a
 * region, and this reads the current contents at build time. The example
 * programs are compiled and run in CI, so a sample that appears in the docs is
 * a sample that works — and changing behaviour in an SDK updates every page
 * that shows it, with no second edit and nothing to forget.
 *
 * Regions are marked in the source with the language's own comment syntax:
 *
 *     // #region init
 *     const mcpd = new Mcpd()
 *     // #endregion init
 */

import fs from 'node:fs'
import path from 'node:path'
import { LANGUAGES, type LanguageId } from './site'

/** Repository root, from this file's location. */
const ROOT = path.resolve(process.cwd(), '..')

const EXAMPLE_DIR: Record<LanguageId, string> = {
  typescript: 'examples/typescript',
  python: 'examples/python',
  rust: 'examples/rust',
  go: 'examples/go',
  kotlin: 'examples/kotlin',
  elixir: 'examples/elixir',
  curl: 'examples/curl',
}

/** The example programs, by the name a docs page asks for. */
export type ExampleName = 'quickstart' | 'workflow'

const EXAMPLE_FILE: Record<ExampleName, Partial<Record<LanguageId, string>>> = {
  quickstart: {
    typescript: 'quickstart.ts',
    python: 'quickstart.py',
    rust: 'quickstart.rs',
    go: 'quickstart.go',
    kotlin: 'Quickstart.kt',
    elixir: 'quickstart.exs',
    curl: 'quickstart.sh',
  },
  // The durable-run programs. No curl: the worker protocol is a loop with
  // state, and a shell that drove it would be an SDK by another name.
  workflow: {
    typescript: 'workflow.ts',
    python: 'workflow.py',
    rust: 'workflow.rs',
    go: 'workflow.go',
    kotlin: 'Workflow.kt',
    elixir: 'workflow.exs',
  },
}

/** The languages an example program exists in. */
export function exampleLanguages(example: ExampleName): LanguageId[] {
  return Object.keys(EXAMPLE_FILE[example]) as LanguageId[]
}

export class SnippetError extends Error {}

/**
 * Read one region from one language's example.
 *
 * Throws rather than returning a placeholder: a docs page that silently renders
 * an empty code block teaches the reader nothing and hides the breakage from
 * whoever removed the region. Failing the build is the point.
 */
export function snippet(lang: LanguageId, region: string, example: ExampleName = 'quickstart'): string {
  const name = EXAMPLE_FILE[example][lang]
  if (!name) {
    throw new SnippetError(`No ${example} example for ${lang}: the docs page must restrict its languages.`)
  }
  const file = path.join(ROOT, EXAMPLE_DIR[lang], name)
  if (!fs.existsSync(file)) {
    throw new SnippetError(
      `No example for ${lang}: expected ${path.relative(ROOT, file)}. ` +
        `Every language in LANGUAGES needs an example program.`,
    )
  }
  const source = fs.readFileSync(file, 'utf8')
  const lines = source.split('\n')

  const startAt = lines.findIndex((l) => l.includes(`#region ${region}`))
  if (startAt === -1) {
    throw new SnippetError(
      `Region "${region}" is missing from ${path.relative(ROOT, file)}. ` +
        `Add "#region ${region}" / "#endregion ${region}" markers around the code the docs should show.`,
    )
  }
  const endOffset = lines.slice(startAt + 1).findIndex((l) => l.includes(`#endregion ${region}`))
  if (endOffset === -1) {
    throw new SnippetError(
      `Region "${region}" in ${path.relative(ROOT, file)} is opened but never closed.`,
    )
  }

  const body = lines.slice(startAt + 1, startAt + 1 + endOffset)

  // Strip the common indentation so a region nested inside a function does not
  // arrive in the docs indented by eight spaces.
  const indents = body
    .filter((l) => l.trim().length > 0)
    .map((l) => l.length - l.trimStart().length)
  const strip = indents.length ? Math.min(...indents) : 0

  return body
    .map((l) => l.slice(strip))
    .join('\n')
    .replace(/^\n+|\s+$/g, '')
}

/** Read the same region from every language, for a switcher. */
export function snippetSet(
  region: string,
  only?: readonly LanguageId[],
  example: ExampleName = 'quickstart',
): { lang: LanguageId; label: string; shiki: string; code: string }[] {
  const wanted = only ?? exampleLanguages(example)
  return LANGUAGES.filter((l) => wanted.includes(l.id)).map((l) => ({
    lang: l.id,
    label: l.label,
    shiki: l.shiki,
    code: snippet(l.id, region, example),
  }))
}
