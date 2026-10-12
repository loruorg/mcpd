/**
 * The site's syntax themes, one per colour scheme.
 *
 * Built from the site's own signal colours rather than imported from a stock
 * theme, so code reads as part of the page. The mapping is the same in both:
 * red is a keyword, green a string, amber a number, blue a call or a type,
 * and comments recede. Both are written into every token by Shiki, and
 * `panel.css` picks one with `light-dark()`, so a code block follows the theme
 * with no script.
 */

import type { ThemeRegistration } from 'shiki'

type Palette = { ink: string; ink2: string; ink3: string; comment: string; oxide: string; olive: string; amber: string; slate: string }

const LIGHT: Palette = {
  ink: '#0f1412', ink2: '#3a433f', ink3: '#59625d', comment: '#6b746f',
  oxide: '#b3312a', olive: '#1f7a43', amber: '#94560a', slate: '#2d5ec2',
}
const DARK: Palette = {
  ink: '#e6eae8', ink2: '#c3cac6', ink3: '#97a09b', comment: '#7b847f',
  oxide: '#ff8a7d', olive: '#7fdca2', amber: '#f2bd62', slate: '#93b4ff',
}

function theme(name: string, type: 'light' | 'dark', c: Palette): ThemeRegistration {
  const { ink, ink2, ink3, comment, oxide, olive, amber, slate } = c
  return {
    name,
    type,
  colors: {
    'editor.background': 'transparent',
    'editor.foreground': ink,
  },
  settings: [
    { scope: ['comment', 'punctuation.definition.comment', 'string.comment'], settings: { foreground: comment } },

    { scope: ['keyword', 'storage', 'storage.type', 'keyword.control', 'keyword.operator.new',
              'keyword.other', 'variable.language', 'constant.language', 'support.type.primitive'],
      settings: { foreground: oxide } },

    { scope: ['string', 'string.quoted', 'string.template', 'constant.other.symbol',
              'punctuation.definition.string', 'meta.embedded.line'],
      settings: { foreground: olive } },

    { scope: ['constant.numeric', 'constant.language.boolean', 'constant.language.null',
              'constant.language.undefined', 'constant.character.escape'],
      settings: { foreground: amber } },

    { scope: ['entity.name.function', 'support.function', 'meta.function-call',
              'entity.name.type', 'entity.name.class', 'support.class', 'support.type',
              'entity.other.attribute-name'],
      settings: { foreground: slate } },

    { scope: ['variable', 'variable.other', 'meta.object-literal.key', 'support.variable'],
      settings: { foreground: ink2 } },

    { scope: ['punctuation', 'meta.brace', 'keyword.operator', 'punctuation.separator',
              'punctuation.terminator'],
      settings: { foreground: ink3 } },

    { scope: ['markup.inserted'], settings: { foreground: olive } },
    { scope: ['markup.deleted'], settings: { foreground: oxide } },
    { scope: ['markup.bold'], settings: { fontStyle: 'bold' } },
    { scope: ['markup.italic'], settings: { fontStyle: 'italic' } },
  ],
  }
}

export const panelTheme = theme('mcpd-light', 'light', LIGHT)
export const panelThemeDark = theme('mcpd-dark', 'dark', DARK)

/** Pass to Shiki or `<Code themes>` together with `defaultColor: false`. */
export const SHIKI_THEMES = { light: panelTheme, dark: panelThemeDark }
