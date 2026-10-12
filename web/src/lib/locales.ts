/**
 * The locale table.
 *
 * This is data and nothing else, with no imports, because `astro.config.mjs`
 * has to read it while Astro is still starting up and cannot resolve a virtual
 * module such as `astro:i18n`. The routing configuration and the site's own
 * helpers therefore read one list rather than two that can drift.
 *
 * Import `@/lib/i18n` instead of this file. It re-exports everything here and
 * adds the helpers, so there is one name to remember.
 */

/**
 * Every locale the site publishes, in the order the picker shows them.
 *
 * `code` is both the URL prefix and the directory name under `src/pages`.
 *
 * `name` is the language named in its own language, because a reader looking
 * for their language cannot read a list written in a language they do not have.
 * They are capitalised even where the language lowercases a language name in
 * running prose (French writes "français", Italian "italiano"): this is a list
 * of labels, not a sentence, and a list that mixes `English` with `français`
 * reads as a mistake rather than as correct French.
 *
 * `tag` is the BCP 47 tag published in `lang` and in `hreflang`. It is not
 * always the code. `zh` is Simplified Chinese, and `zh-Hans` is the difference
 * between a Simplified reader being offered the page and a Traditional reader
 * being offered it by mistake. `en` stays unqualified on purpose: the source is
 * British English, but `en-GB` would tell a search engine to withhold the page
 * from every other English speaker.
 *
 * `dir` is the writing direction. All nine of these are left to right. The
 * field exists so that the day a right-to-left locale is added the layout
 * already sets `dir` from data instead of growing its first conditional.
 */
export const LOCALES = [
  { code: 'en', name: 'English',   tag: 'en',      dir: 'ltr' },
  { code: 'fr', name: 'Français',  tag: 'fr',      dir: 'ltr' },
  { code: 'es', name: 'Español',   tag: 'es',      dir: 'ltr' },
  { code: 'zh', name: '中文',       tag: 'zh-Hans', dir: 'ltr' },
  { code: 'ja', name: '日本語',      tag: 'ja',      dir: 'ltr' },
  { code: 'ko', name: '한국어',      tag: 'ko',      dir: 'ltr' },
  { code: 'it', name: 'Italiano',  tag: 'it',      dir: 'ltr' },
  { code: 'tr', name: 'Türkçe',    tag: 'tr',      dir: 'ltr' },
  { code: 'pt', name: 'Português', tag: 'pt',      dir: 'ltr' },
] as const satisfies readonly {
  code: string
  name: string
  tag: string
  dir: 'ltr' | 'rtl'
}[]

/** One of the nine codes. Use this, never a bare `string`, for a locale. */
export type Locale = (typeof LOCALES)[number]['code']

/** What one row of the table looks like. */
export type LocaleInfo = (typeof LOCALES)[number]

/** The unprefixed locale. Its pages keep the URLs the site already publishes. */
export const DEFAULT_LOCALE: Locale = 'en'

/** The codes on their own, in picker order, for iteration. */
export const LOCALE_CODES: readonly Locale[] = LOCALES.map((l) => l.code)

const BY_CODE = new Map<string, LocaleInfo>(LOCALES.map((l) => [l.code, l]))

/** Whether an arbitrary string is one of the nine. Narrows the type. */
export function isLocale(value: string | undefined | null): value is Locale {
  return value != null && BY_CODE.has(value)
}

/** Everything known about one locale. Throws on an unknown code, which is a bug. */
export function locale(code: Locale): LocaleInfo {
  const found = BY_CODE.get(code)
  if (!found) {
    throw new Error(`unknown locale "${code}": expected one of ${LOCALE_CODES.join(', ')}`)
  }
  return found
}
