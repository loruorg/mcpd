/**
 * What the run list shows, chosen by the reader.
 *
 * A dialog rather than a menu, for one reason: adding a metadata column means
 * typing a key, and a text field inside a menu fights the menu's own typeahead.
 * The dialog has room to say what a column means, which matters here because
 * several of them are figures that are easy to read as something they are not.
 *
 * Radix supplies the behaviour: the focus trap, the return of focus to the
 * trigger, Escape, the outside press, and the scroll lock. The surface is the
 * panel's own.
 */

import { useState } from 'react'
import type { ReactNode } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import type { Trace } from '@/generated/types'
import { COLUMNS, DEFAULT_PREFS, metaKeys } from './list-prefs'
import type { Density, ListPrefs } from './list-prefs'
import { CrossMark, ValueField } from './list-ui'

export function ColumnDialog({
  prefs,
  onPrefs,
  onReset,
  traces,
}: {
  prefs: ListPrefs
  onPrefs: (p: ListPrefs) => void
  onReset: () => void
  /** The runs on screen, whose keys are what a metadata column can be added by. */
  traces: Trace[]
}) {
  const [key, setKey] = useState('')
  const hidden = new Set(prefs.hidden)
  const optional = COLUMNS.filter((c) => !c.fixed)
  const fixed = COLUMNS.length - optional.length
  // What the trigger says is what is on screen, which includes the three that
  // cannot be turned off. A count of the optional ones only reads as wrong.
  const shown = fixed + optional.filter((c) => !hidden.has(c.id)).length + prefs.meta.length

  const toggle = (id: string) => {
    const next = new Set(hidden)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    onPrefs({ ...prefs, hidden: [...next] })
  }

  const suggestions = metaKeys(traces).filter((k) => !prefs.meta.includes(k))

  const addMeta = (raw: string) => {
    const k = raw.trim()
    if (!k || prefs.meta.includes(k)) return
    // Eight is as many as the header can carry before the run name, which is the
    // only column that takes the space left over, has none left to take.
    if (prefs.meta.length >= 8) return
    onPrefs({ ...prefs, meta: [...prefs.meta, k] })
    setKey('')
  }

  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <button
          type="button"
          title="Choose the columns this list shows"
          className="legend panel-raised shrink-0 px-2 py-1 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink data-[state=open]:bg-panel-3 data-[state=open]:text-ink"
        >
          columns {shown}
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        {/* An honest scrim: the panel's own ink, no blur. */}
        <Dialog.Overlay className="fixed inset-0 z-50" style={{ background: 'var(--scrim)' }} />
        <Dialog.Content
          aria-describedby="mcpd-columns-note"
          className="panel-raised fixed left-1/2 top-1/2 z-50 grid max-h-[min(42rem,calc(100vh-3rem))] w-[min(36rem,calc(100vw-1.5rem))] -translate-x-1/2 -translate-y-1/2 grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden"
          style={{ background: 'var(--color-panel-2)', boxShadow: 'var(--shadow-float)' }}
        >
          <header className="flex items-baseline gap-4 border-b border-scribe-2 px-5 py-3.5">
            <Dialog.Title className="display text-[1.0625rem]">Columns</Dialog.Title>
            <p id="mcpd-columns-note" className="max-w-[26ch] text-[0.75rem] leading-snug text-ink-3">
              Held in this browser only. Created, Run and Status are always shown.
            </p>
            <Dialog.Close
              aria-label="Close"
              title="Close"
              className="ml-auto grid h-5 w-5 place-items-center text-ink-3 transition-colors hover:text-ink"
            >
              <CrossMark size={10} />
            </Dialog.Close>
          </header>

          <div className="overflow-y-auto px-5 pb-6 pt-4">
            <Section label="Density" inline>
              <div className="flex items-center gap-1.5">
                {(['compact', 'roomy'] as Density[]).map((d) => (
                  <button
                    key={d}
                    type="button"
                    onClick={() => onPrefs({ ...prefs, density: d })}
                    aria-pressed={prefs.density === d}
                    className={`legend panel-raised px-2.5 py-1 text-[0.5625rem] transition-colors ${
                      prefs.density === d
                        ? 'bg-panel-3 text-ink'
                        : 'text-ink-3 hover:bg-panel-3 hover:text-ink'
                    }`}
                  >
                    {d}
                  </button>
                ))}
              </div>
            </Section>

            <Section label="Optional">
              <ul className="grid gap-x-5 gap-y-0.5 sm:grid-cols-3">
                {optional.map((c) => (
                  <li key={c.id}>
                    <Switch
                      on={!hidden.has(c.id)}
                      onChange={() => toggle(c.id)}
                      title={c.help}
                      label={c.label}
                    />
                  </li>
                ))}
              </ul>
            </Section>

            <Section label="By key">
              <p className="max-w-[52ch] text-[0.8125rem] leading-relaxed text-ink-2">
                A column of your own, read from each run’s correlation, then its
                score means, then its feedback counts.
              </p>
              {prefs.meta.length > 0 && (
                <ul className="mt-2.5 flex flex-wrap items-center gap-2">
                  {prefs.meta.map((k) => (
                    <li key={k} className="well flex items-center gap-2 py-1 pl-2.5 pr-1.5">
                      <span className="data text-[0.8125rem] text-ink">{k}</span>
                      <button
                        type="button"
                        onClick={() => onPrefs({ ...prefs, meta: prefs.meta.filter((x) => x !== k) })}
                        aria-label={`Remove the ${k} column`}
                        title={`Remove the ${k} column`}
                        className="grid h-4 w-4 place-items-center text-ink-3 transition-colors hover:text-oxide"
                      >
                        <CrossMark />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <div className="mt-2.5 flex items-center gap-2">
                <div className="well flex min-w-0 items-center gap-2 py-1 pl-2.5 pr-1.5">
                  <span className="legend shrink-0 text-[0.5625rem] text-ink-3">key</span>
                  <ValueField
                    value={key}
                    onCommit={addMeta}
                    onTyping={setKey}
                    label="Metadata key"
                    placeholder="release"
                    width="w-[10rem]"
                    discovery={
                      suggestions.length
                        ? { values: suggestions, state: 'ready', note: null }
                        : {
                            values: [],
                            state: 'ready',
                            note: null,
                          }
                    }
                  />
                </div>
                <button
                  type="button"
                  onClick={() => addMeta(key)}
                  disabled={!key.trim() || prefs.meta.length >= 8}
                  title={
                    prefs.meta.length >= 8
                      ? 'Eight columns of your own is the most the header can carry'
                      : 'Add this key as a column'
                  }
                  className="legend panel-raised px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink disabled:cursor-not-allowed disabled:border-transparent disabled:bg-transparent disabled:hover:bg-transparent"
                >
                  add column
                </button>
              </div>
              <p className="mt-2 text-[0.75rem] leading-snug text-ink-3">
                {suggestions.length
                  ? `Keys the runs on screen carry: ${suggestions.slice(0, 6).join(', ')}${suggestions.length > 6 ? ', and more' : ''}.`
                  : 'The runs on screen carry no further keys. A key can still be typed in full.'}
              </p>
            </Section>
          </div>

          <footer className="flex items-center gap-3 border-t border-scribe-2 px-5 py-3">
            <button
              type="button"
              onClick={onReset}
              title="Put every column, the density and the split back to how they started"
              className="legend px-1 py-1 text-[0.5625rem] text-ink-3 transition-colors hover:text-oxide"
            >
              reset to defaults
            </button>
            <span className="ml-auto text-[0.75rem] text-ink-3">
              {shown} of {COLUMNS.length + prefs.meta.length} shown
            </span>
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Section({
  label,
  children,
  inline,
}: {
  label: string
  children: ReactNode
  inline?: boolean
}) {
  return (
    <section className={inline ? 'mb-4 flex items-center gap-3' : 'mb-5 last:mb-0'}>
      <h3 className={`legend text-ink-3 ${inline ? '' : 'mb-2'}`}>{label}</h3>
      {children}
    </section>
  )
}

function Switch({
  on,
  onChange,
  label,
  title,
}: {
  on: boolean
  onChange: () => void
  label: string
  title: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={onChange}
      title={title}
      className="flex w-full items-center gap-2 py-1 text-left text-[0.8125rem] text-ink-2 transition-colors hover:text-ink"
    >
      <span
        aria-hidden="true"
        className="inline-block h-2.5 w-2.5 shrink-0 rounded-[1px] border border-scribe"
        style={{ background: on ? 'var(--color-ink)' : 'var(--color-well)' }}
      />
      <span className="min-w-0 truncate">{label}</span>
    </button>
  )
}

export { DEFAULT_PREFS }
