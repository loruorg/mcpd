/**
 * Language on this site: the nine locales, and everything routing needs.
 *
 * Import this file, not `@/lib/locales`. It re-exports the locale table and
 * adds the four helpers that read and build a URL. The layout reads it to set
 * `lang` and publish alternates, the rail reads it to build the picker, and
 * every workflow that adds a translation reads it to know where a page goes.
 *
 * ## The scheme
 *
 * English stays exactly where it is. `/docs/quickstart` is English and always
 * was, so no published link breaks. Every other locale takes a prefix, and its
 * pages sit under a matching directory:
 *
 *     src/pages/docs/quickstart.astro     ->  /docs/quickstart      (en)
 *     src/pages/fr/docs/quickstart.astro  ->  /fr/docs/quickstart   (fr)
 *
 * Astro's own i18n routing does the URL work, configured in `astro.config.mjs`
 * with `prefixDefaultLocale: false`. Nothing here reinvents it: `urlFor` is a
 * typed wrapper over `getRelativeLocaleUrl`, so a caller cannot build a path by
 * hand and get the default locale's absent prefix wrong.
 *
 * ## Casing, and the Turkish trap
 *
 * `toLowerCase` and `toUpperCase` are locale-invariant: they always apply the
 * Unicode default mapping. `toLocaleLowerCase` and `toLocaleUpperCase` apply
 * the reader's language rules, and Turkish rules differ: `I` lowercases to `ı`
 * (dotless) and `i` uppercases to `İ` (dotted). The two are not
 * interchangeable, and choosing the wrong one breaks something either way.
 *
 * What decides is what the string is FOR, never where it sits:
 *
 * - Normalising a slug, an id, a CSS class, a lookup key or a search index:
 *   use `toLowerCase()`. Locale-aware folding would make a Turkish reader's
 *   search for `Init` miss the span named `init`, because their `I` folds to
 *   `ı` while the index holds `i`.
 * - Producing text a person reads: use `capitalise()` below, or
 *   `toLocaleUpperCase(tag)`, so Turkish reads correctly.
 *
 * Every case call in this repository carries a comment saying which of the two
 * it is and why. CSS needs no equivalent: `text-transform` is defined as
 * language-sensitive and browsers take the language from `lang`, which the
 * layout now sets per locale.
 */

import { getRelativeLocaleUrl } from 'astro:i18n'
import { routePath } from '@/lib/site'
import { DEFAULT_LOCALE, LOCALE_CODES, isLocale, locale, type Locale } from '@/lib/locales'

export {
  LOCALES,
  LOCALE_CODES,
  DEFAULT_LOCALE,
  isLocale,
  locale,
  type Locale,
  type LocaleInfo,
} from '@/lib/locales'

/**
 * The locale a URL is in.
 *
 * Pass `Astro.url.pathname` straight in. This deliberately does not use
 * `Astro.currentLocale`: the site builds with `build.format: 'file'`, so at
 * build time a locale's home page has the pathname `/fr.html`, whose first
 * segment is `fr.html` and not `fr`. `Astro.currentLocale` is undefined there,
 * and the French home page would go out labelled English. Cleaning the path
 * first is what makes every page agree.
 */
export function localeOf(pathname: string): Locale {
  const first = routePath(pathname).split('/')[1]
  return isLocale(first) ? first : DEFAULT_LOCALE
}

/**
 * The route a URL names, with its locale prefix removed and `.html` cleaned off.
 *
 * `/fr/docs/quickstart` and `/docs/quickstart` both give `/docs/quickstart`, so
 * a navigation list written once in English can be compared and localised for
 * every locale. Any locale's home page gives `/`.
 */
export function routeOf(pathname: string): string {
  const path = routePath(pathname)
  const first = path.split('/')[1]
  if (!isLocale(first) || first === DEFAULT_LOCALE) return path
  const rest = path.slice(first.length + 1)
  return rest === '' ? '/' : rest
}

/**
 * The URL of one route in one locale.
 *
 * `urlFor('/docs/quickstart', 'fr')` is `/fr/docs/quickstart`; the same route
 * in `en` is `/docs/quickstart`, with no prefix. Pass the unprefixed route,
 * which is what `routeOf` returns.
 */
export function urlFor(route: string, code: Locale): string {
  const url = getRelativeLocaleUrl(code, route === '/' ? '' : route)
  // `trailingSlash: 'never'` governs the pages, but the helper still returns
  // `/fr/` for a locale root. A canonical URL that disagrees with the routing
  // config is how one page ends up indexed under two addresses.
  const trimmed = url.length > 1 ? url.replace(/\/$/, '') : url
  return trimmed === '' ? '/' : trimmed
}

/**
 * Sentence-case the first character of text a person reads.
 *
 * Locale-aware on purpose: this is display text, so a Turkish `istek` has to
 * become `İstek` and not `Istek`. Never use it on a slug, an id or a key.
 */
export function capitalise(text: string, code: Locale): string {
  if (text === '') return text
  const first = String.fromCodePoint(text.codePointAt(0)!)
  return first.toLocaleUpperCase(locale(code).tag) + text.slice(first.length)
}

/*
 * Which locales actually hold a page.
 *
 * Translations land one page at a time, so a route exists in some locales and
 * not others for a long while. An `hreflang` alternate for a page nobody has
 * written points a search engine at a 404, so the layout asks this rather than
 * assuming all nine.
 *
 * The answer is read from the page files themselves at build time. Nothing
 * registers a translation anywhere: dropping `src/pages/fr/docs/cost.astro`
 * into the tree is what gives `/docs/cost` a French alternate, in the head of
 * every page, in the sitemap and in the picker. A registry would only be a
 * second place to forget.
 *
 * Vite resolves `import.meta.glob` at build time and only its keys are read
 * here, so no page module is loaded and there is no import cycle back into the
 * layout that calls this.
 */
const PAGE_FILES = Object.keys(
  import.meta.glob(['/src/pages/**/*.astro', '/src/pages/**/*.md', '/src/pages/**/*.mdx']),
)

const ROUTES_BY_LOCALE: ReadonlyMap<string, ReadonlySet<Locale>> = (() => {
  const map = new Map<string, Set<Locale>>()
  for (const file of PAGE_FILES) {
    const rel = file.replace(/^\/src\/pages\//, '').replace(/\.(astro|md|mdx)$/, '')
    // Astro serves no route for a segment starting with `_`, and a dynamic
    // route has no single URL to publish an alternate for.
    if (rel.includes('[') || rel.split('/').some((s) => s.startsWith('_'))) continue
    const parts = rel.split('/')
    const head = parts[0]
    const code: Locale = isLocale(head) && head !== DEFAULT_LOCALE ? head : DEFAULT_LOCALE
    const rest = code === DEFAULT_LOCALE ? parts : parts.slice(1)
    const trimmed = rest[rest.length - 1] === 'index' ? rest.slice(0, -1) : rest
    const route = trimmed.length === 0 ? '/' : `/${trimmed.join('/')}`
    const set = map.get(route) ?? new Set<Locale>()
    set.add(code)
    map.set(route, set)
  }
  return map
})()

/**
 * The locales this route has a written page in, in picker order.
 *
 * A route the glob somehow missed still publishes itself as English, so the
 * head of a page is never empty of alternates.
 */
export function localesWithPage(route: string): Locale[] {
  const have = ROUTES_BY_LOCALE.get(route)
  if (!have) return [DEFAULT_LOCALE]
  return LOCALE_CODES.filter((c) => have.has(c))
}
