/** Facts about the site that more than one page needs. Single source. */

export const SITE = {
  name: 'mcpd',
  url: 'https://mcpd.sh',
  /** Used as the default share description and the meta description fallback. */
  tagline: 'Durable runs and observability for agents, in one binary.',
  description:
    'A free, open-source daemon for AI agents. It holds a workflow between steps while ' +
    'your own code, in any language, does the work inside each one, and records every ' +
    'step as a trace with its cost.',
  repo: 'https://github.com/mcpd/mcpd',
  licence: 'Apache-2.0',
  /** The port mcpd listens on by default. Referenced in a lot of copy. */
  port: 4317,
} as const

/**
 * The PROGRAMMING languages the docs can switch between, in the order shown.
 *
 * Not the reader's language. The nine human locales live in `@/lib/i18n`, and
 * these two lists never mix: a Japanese reader still chooses between TypeScript
 * and Go, and this list is the same in all nine locales.
 */
export const LANGUAGES = [
  { id: 'typescript', label: 'TypeScript', shiki: 'typescript', ext: 'ts' },
  { id: 'python', label: 'Python', shiki: 'python', ext: 'py' },
  { id: 'rust', label: 'Rust', shiki: 'rust', ext: 'rs' },
  { id: 'go', label: 'Go', shiki: 'go', ext: 'go' },
  { id: 'kotlin', label: 'Kotlin', shiki: 'kotlin', ext: 'kt' },
  { id: 'elixir', label: 'Elixir', shiki: 'elixir', ext: 'exs' },
  { id: 'curl', label: 'cURL', shiki: 'bash', ext: 'sh' },
] as const

export type LanguageId = (typeof LANGUAGES)[number]['id']


/**
 * The canonical route for a page, from `Astro.url.pathname`.
 *
 * The site builds with `build.format: 'file'`, so at build time the pathname is
 * `/docs/quickstart.html`, not `/docs/quickstart`. Comparing that against the
 * hrefs in the navigation silently never matched: no active state was emitted
 * anywhere, the previous and next links resolved to nothing on every
 * documentation page, and canonical URLs went out with a `.html` suffix that
 * contradicted `trailingSlash: 'never'`. Everything that compares or publishes
 * a route goes through this.
 *
 * It knows nothing about locales, on purpose: `/fr/docs/quickstart` cleans to
 * `/fr/docs/quickstart`. Stripping the prefix is `routeOf` in `@/lib/i18n`,
 * which calls this first.
 */
export function routePath(pathname: string): string {
  const cleaned = pathname.replace(/\.html$/, '').replace(/\/index$/, '')
  return cleaned === '' ? '/' : cleaned
}
