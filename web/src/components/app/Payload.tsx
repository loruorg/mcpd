/**
 * One payload section: the prompt, the completion, the attributes or the
 * metadata a span carries.
 *
 * Three things make this more than a `<pre>`. It can be searched on its own, so
 * a query aimed at one prompt does not filter the whole trace. It can be read
 * in either of the two forms a payload actually arrives in, escaped on one line
 * as it went over the wire or laid out over many. And it will not block the
 * panel: a payload is the largest thing mcpd stores, and formatting a
 * ten-megabyte prompt into the DOM freezes the tab that was trying to read it.
 */

import { useEffect, useMemo, useRef, useState } from 'react'
import { Action, CopyValue, Highlight, SearchField, countMatches } from './trace-ui'

/** How much of a payload is rendered before the reader asks for more. */
const CHUNK = 24_000

/**
 * The length past which a section carries a search box of its own.
 *
 * Four sections each with a field is four fields on the panel, and three of
 * them would be searching a payload you can read whole without scrolling. The
 * box appears where there is something it could find that you cannot already
 * see.
 */
const SEARCHABLE = 320

/** Beyond this, the pretty form is not built at all: see `formsOf`. */
const PRETTY_CAP = 2_000_000

type Form = 'messages' | 'pretty' | 'escaped'

interface Message {
  role?: string
  content?: unknown
}

/**
 * The forms this value can be read in, and the text of each.
 *
 * The escaped form is built first because it is the cheap one, and it decides
 * whether the pretty form is built at all: `JSON.stringify(value, null, 2)` on
 * a payload of a few megabytes is seconds of blocked main thread for a shape
 * nobody can read anyway.
 */
function formsOf(value: unknown): { escaped: string; pretty: string | null; messages: Message[] | null } {
  let escaped: string
  try {
    escaped = typeof value === 'string' ? value : JSON.stringify(value) ?? String(value)
  } catch {
    escaped = String(value)
  }
  let pretty: string | null = null
  if (escaped.length <= PRETTY_CAP) {
    try {
      pretty = typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? escaped
    } catch {
      pretty = null
    }
  }
  const messages =
    value != null &&
    typeof value === 'object' &&
    !Array.isArray(value) &&
    Array.isArray((value as { messages?: unknown }).messages)
      ? ((value as { messages: Message[] }).messages)
      : null
  return { escaped, pretty, messages }
}

/**
 * Cut a string at a line boundary, so a chunk never ends mid-token.
 *
 * Two rules keep the cut from being sillier than the text it withholds. A
 * remainder smaller than a tenth of a chunk is rendered instead of held back:
 * withholding two lines and offering two controls to reveal them costs more
 * attention than the two lines ever would. And the line boundary is only used
 * when it is close to the target, because a prompt is often one line thirty
 * thousand characters long, and running to the end of it renders the whole
 * payload while claiming to have paged it.
 */
function chunkOf(text: string, chunks: number): string {
  const want = CHUNK * chunks
  if (text.length <= want + CHUNK * 0.1) return text
  const nl = text.indexOf('\n', want)
  return text.slice(0, nl !== -1 && nl - want <= CHUNK * 0.2 ? nl : want)
}

export function Payload({
  label,
  value,
  /** The panel's own search, so one query paints the waterfall and this together. */
  traceQuery,
  spanId,
}: {
  label: string
  value: unknown
  traceQuery: string
  spanId: string
}) {
  const [form, setForm] = useState<Form | null>(null)
  const [own, setOwn] = useState('')
  const [chunks, setChunks] = useState(1)
  // Tall lifts the box's height cap to most of the window, for a long prompt
  // that is read rather than skimmed.
  const [tall, setTall] = useState(false)
  const pre = useRef<HTMLDivElement>(null)

  const { escaped, pretty, messages } = useMemo(() => formsOf(value), [value])

  // A different span swapped into the same section is a different payload, so
  // its own search, its unfolded length and its chosen form all start again.
  // Keeping them would apply one prompt's query to the next prompt's text and
  // scroll to a match that is not there.
  useEffect(() => {
    setOwn('')
    setChunks(1)
    setForm(null)
  }, [spanId, label])

  const active: Form = form ?? (messages ? 'messages' : pretty != null ? 'pretty' : 'escaped')
  const query = own.trim() || traceQuery.trim()
  const full = active === 'escaped' ? escaped : (pretty ?? escaped)
  const shown = useMemo(() => chunkOf(full, chunks), [full, chunks])

  // The message view pages the same way the code view does. A single message
  // can be the whole payload, and laying a megabyte of prompt into one text
  // node is the same freeze by another route.
  const bubbles = useMemo(
    () =>
      (messages ?? []).map((m) => {
        const text =
          typeof m.content === 'string' ? m.content : (JSON.stringify(m.content, null, 2) ?? '')
        return { role: m.role, text, shown: chunkOf(text, chunks) }
      }),
    [messages, chunks],
  )

  const hidden =
    active === 'messages'
      ? bubbles.reduce((n, b) => n + (b.text.length - b.shown.length), 0)
      : full.length - shown.length

  // The first match, brought into the section's own scroller rather than by
  // `scrollIntoView`: that would scroll every ancestor as well, and pull the
  // whole inspector, and sometimes the page, out from under the reader.
  useEffect(() => {
    const box = pre.current
    if (!box || !query) return
    const mark = box.querySelector<HTMLElement>('[data-first-match]')
    if (!mark) return
    const top = mark.offsetTop - box.clientHeight / 2 + mark.offsetHeight / 2
    box.scrollTop = Math.max(0, top)
  }, [query, shown, active, spanId])

  if (value == null) return null

  // Counted over the whole payload and over the part rendered, because those
  // are two different facts and a reader hunting a string needs both: a count
  // that silently means "in the part we drew" sends them away from a payload
  // that does hold what they asked for.
  const rendered = active === 'messages' ? bubbles.map((b) => b.shown).join('\n') : shown
  const inFull = query ? countMatches(full, query) : 0
  const inShown = query ? (hidden > 0 ? countMatches(rendered, query) : inFull) : 0

  return (
    <section className="mt-5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <h4 className="legend text-ink-3">{label}</h4>
        <span className="data text-[0.625rem] text-ink-3">{size(full.length)}</span>
        {query && (
          <span className="data text-[0.625rem] text-ink-3">
            {inFull === 0
              ? 'no match here'
              : inShown === inFull
                ? `${inFull} match${inFull === 1 ? '' : 'es'} here`
                : `${inShown} of ${inFull} matches rendered`}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          {messages && (
            <Action
              onClick={() => setForm('messages')}
              pressed={active === 'messages'}
              title="Read the messages as text"
            >
              messages
            </Action>
          )}
          {pretty != null && (
            <Action
              onClick={() => setForm('pretty')}
              pressed={active === 'pretty'}
              title="Laid out over many lines"
            >
              multiline
            </Action>
          )}
          <Action
            onClick={() => setForm('escaped')}
            pressed={active === 'escaped'}
            title="One line, escaped, as it went over the wire"
          >
            escaped
          </Action>
          <Action
            onClick={() => setTall((t) => !t)}
            pressed={tall}
            title={tall ? 'Back to the short box' : 'Read it at the height of the window'}
          >
            {tall ? 'shrink' : 'expand'}
          </Action>
          <CopyValue value={full} className="legend text-[0.5625rem] text-ink-3 hover:text-ink">
            <span className="legend text-[0.5625rem]">copy</span>
          </CopyValue>
        </div>
      </div>

      {full.length > SEARCHABLE && (
        <SearchField
          className="mt-2"
          value={own}
          onChange={setOwn}
          label={`Search ${label.toLowerCase()}`}
          placeholder={
            traceQuery
              ? `Search ${label.toLowerCase()}, or clear to keep the run search`
              : `Search ${label.toLowerCase()}`
          }
        />
      )}

      {active === 'messages' && messages ? (
        <div ref={pre} className={`mt-2 ${tall ? 'max-h-[80vh]' : 'max-h-80'} overflow-y-auto pr-1`}>
          <div className="space-y-2">
            {bubbles.map((b, i) => (
              <div key={i} className="well rounded-[2px] p-2.5">
                <p className="legend text-ink-3">{b.role ?? 'message'}</p>
                <p className="mt-1 whitespace-pre-wrap break-words text-[0.8125rem] leading-relaxed text-ink">
                  <Highlight text={b.shown} query={query} />
                </p>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div
          ref={pre}
          className={`well data mt-2 ${tall ? 'max-h-[80vh]' : 'max-h-80'} overflow-auto rounded-[2px] p-2.5 text-[0.75rem] leading-relaxed text-ink ${
            active === 'escaped' ? 'whitespace-pre-wrap break-all' : 'scroll-x whitespace-pre'
          }`}
        >
          <Highlight text={shown} query={query} />
        </div>
      )}

      {/* Under the box and not inside it. Inside, the control to render more sat
          at the end of twenty-four thousand characters, which is a control you
          have to scroll the whole payload to reach in order to be shown more of
          it. */}
      {hidden > 0 && (
        <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1.5">
          <span className="text-[0.6875rem] text-ink-3">
            {size(hidden)} of this payload is not rendered yet.
          </span>
          <Action onClick={() => setChunks((c) => c + 1)} title="Render the next part">
            show more
          </Action>
          <Action
            onClick={() =>
              setChunks(
                Math.ceil(
                  Math.max(full.length, ...bubbles.map((b) => b.text.length), 1) / CHUNK,
                ),
              )
            }
            title="Render all of it"
          >
            show all
          </Action>
        </div>
      )}

      {hidden > 0 && query && inShown === 0 && inFull > 0 && (
        <p className="mt-1.5 text-[0.75rem] leading-snug text-ink-2">
          Every match is further down than the part rendered. Show more to reach it.
        </p>
      )}
    </section>
  )
}



/** A payload's size in the units a reader thinks in. */
function size(chars: number): string {
  if (chars < 1000) return `${chars} chars`
  if (chars < 1_000_000) return `${(chars / 1000).toFixed(chars < 10_000 ? 1 : 0)}k chars`
  return `${(chars / 1_000_000).toFixed(1)}M chars`
}
