// @ts-check
import fs from 'node:fs'
import { defineConfig } from 'astro/config'
import react from '@astrojs/react'
import sitemap from '@astrojs/sitemap'
import tailwind from '@tailwindcss/vite'
import { SHIKI_THEMES } from './src/lib/shiki-theme'
import { LOCALES, LOCALE_CODES, DEFAULT_LOCALE } from './src/lib/locales'

export const SITE = 'https://mcpd.sh'

/* The hreflang code published for each URL prefix. The prefix and the code are
   not always the same string: `/zh/` is Simplified Chinese and says so as
   `zh-Hans`. Built from the one locale table so the sitemap cannot disagree
   with the `lang` attribute the layout writes. */
const sitemapLocales = Object.fromEntries(LOCALES.map((l) => [l.code, l.tag]))

/* The blog index joins the sitemap with the first post. Until then it is one
   line under `noindex`, and listing it would ask a crawler to index a page that
   tells it not to. */
const blogHasPosts = fs
  .readdirSync(new URL('./src/content/blog/', import.meta.url))
  .some((f) => f.endsWith('.md'))

export default defineConfig({
  site: SITE,
  trailingSlash: 'never',
  build: { format: 'file' },

  /* Astro 7 changed the default from `true` to `'jsx'`. In `'jsx'` mode the
     whitespace beside an expression is dropped as well, so a count and the
     word after it run together: "37checks", "specification1.0.0". Built both
     ways during the upgrade, twenty pages changed. `true` is what the site
     shipped with before Astro 7, and it is pinned so the compiler cannot
     change a rendered sentence. */
  compressHTML: true,

  /* Nine locales, English unprefixed.
     `prefixDefaultLocale: false` is what keeps every English URL exactly where
     it is: `/docs/quickstart` stays `/docs/quickstart`, and a translation lives
     beside it under `src/pages/<code>/`. No published link breaks.
     There is deliberately no `fallback`. A fallback would answer `/fr/docs/cost`
     with the English page before anyone has written the French one, and the
     reader would get English under a French URL with a French `lang`. Until the
     page exists the URL does not, and the layout publishes no alternate for it. */
  i18n: {
    defaultLocale: DEFAULT_LOCALE,
    locales: [...LOCALE_CODES],
    routing: { prefixDefaultLocale: false },
  },

  integrations: [
    react(),
    sitemap({
      // The operator UI is a live application, not a document. Indexing it
      // would put someone else's empty dashboard in a search result.
      // Matched on the path, so a post slug that starts with "app" stays in.
      filter: (page) => {
        const path = new URL(page).pathname.replace(/\/$/, '')
        if (path === '/app' || path.startsWith('/app/') || path.endsWith('/404')) return false
        return blogHasPosts || path !== '/blog'
      },
      // Emits the xhtml:link alternates. The integration builds each group from
      // the pages that were actually built, so a route translated into three
      // locales lists three alternates and never points at a page nobody wrote.
      i18n: {
        defaultLocale: DEFAULT_LOCALE,
        locales: sitemapLocales,
      },
      /* One known cosmetic difference, checked and left alone. The home page's
         own alternate is written `https://mcpd.sh/` while its `<loc>` and the
         layout's canonical say `https://mcpd.sh`. An empty path and `/` are
         the same resource under RFC 3986, and the sitemap library re-runs
         `new URL()` over every alternate AFTER the `serialize` hook, so a
         `serialize` that trimmed it would be a no-op dressed as a fix. No other
         URL is affected: only the origin root gains the slash. */
    }),
  ],
  vite: {
    plugins: [tailwind()],
  },
  markdown: {
    shikiConfig: {
      // Two themes, built from this site's own signal colours. A stock theme
      // would drop a second, unrelated colour world into every page.
      themes: SHIKI_THEMES,
      defaultColor: false,
      wrap: false,
    },
  },
})
