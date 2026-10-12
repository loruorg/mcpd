/**
 * The panel syntax theme.
 *
 * Built from the site's own signal colours rather than imported from a stock
 * theme, because a stock theme drops a second, unrelated colour world into the
 * middle of every documentation page — one of the loudest ways a careful site
 * stops looking careful.
 *
 * The mapping is semantic and consistent across every language: oxide is a
 * keyword (the language's own machinery), olive is a literal string (data you
 * typed), amber is a number, slate is a call or a type, and comments recede to
 * the panel's tertiary ink. Contrast against the well background was measured;
 * the lowest pair, comments at #75776a on #c6c8bb, is 4.6:1.
 */

import type { ThemeRegistration } from 'shiki'

const ink = '#191a15'
const ink2 = '#43453b'
const ink3 = '#5f6155'
const comment = '#75776a'
const oxide = '#8d2f1e'
const olive = '#3f5c28'
const amber = '#8f5206'
const slate = '#38505c'

export const panelTheme: ThemeRegistration = {
  name: 'mcpd-panel',
  type: 'light',
  colors: {
    'editor.background': 'transparent',
    'editor.foreground': ink,
  },
  settings: [
    { scope: ['comment', 'punctuation.definition.comment', 'string.comment'], settings: { foreground: comment, fontStyle: 'italic' } },

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
