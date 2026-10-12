/**
 * The conversation screen: one run read back as what was said, not as spans.
 *
 * The waterfall answers "what did this run do". This screen answers the other
 * question people ask of a trace, which is "what did it actually say", and it
 * answers it from the same spans with nothing added to the wire. The
 * reconstruction lives in `conversation.ts`; this file is the reading of it,
 * and almost all of the work here is typographic: a measure, a hanging margin
 * carrying the clock, and the space that separates one exchange from the next.
 *
 * The same run can be read as steps without leaving the screen, because the
 * two readings are of one thing. The step behind any turn is a control: press
 * it and the waterfall opens on that step.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { api, ago, capabilityNote, duration, money, tokens } from '@/lib/api'
import type { Trace, TraceDetail } from '@/generated/types'
import { Disconnected, useDebounced } from './Explorer'
import { DaemonTile, ProjectPicker, useConnection } from './shell'
import { useCapabilities } from './capabilities'
import { TracePanel } from './TracePanel'
import { Action, CopyValue, SearchField } from './trace-ui'
import { readConversation } from './conversation'
import type { Conversation, Turn, Voice } from './conversation'

/** How many runs the list holds. The same page size the explorer asks for. */
const PAGE = 60

/** Characters of one turn shown before the reader asks for the rest. */
const LONG = 1400

/**
 * The query this screen sends.
 *
 * `api.traces` types fifteen of the daemon's twenty-one parameters and
 * `thread_id` is not among them, though the route reads it and the spec
 * documents it. The client serialises whatever it is handed, so declaring the
 * shape here keeps this file type-checked without widening a signature that
 * belongs to another screen's file. `Logs.tsx` does the same, for the same
 * reason.
 */
export interface TraceQuery {
  project: string
  q?: string
  thread_id?: string
  sort?: string
  order?: string
  limit?: number
}

/* --------------------------------------------------------------- voice --- */

/*
 * A voice is coloured with the same five inks the waterfall uses for the kinds
 * behind it: a model speaks in slate, a tool in amber, a fault in oxide. A
 * reader who has learned the legend on the waterfall reads this for free, and
 * nothing here is a colour used as decoration.
 */
const VOICE_INK: Record<Voice, string> = {
  user: 'text-ink',
  assistant: 'text-slate',
  system: 'text-ink-3',
  tool: 'text-amber',
  handoff: 'text-ink-2',
  fault: 'text-oxide',
  note: 'text-ink-3',
}

/*
 * A standing instruction is not speech, and it is usually the longest thing in
 * the run. Setting it a step down in size and ink puts it where it belongs in
 * the hierarchy without hiding it, so a two-hundred-line system prompt does not
 * shout over the one sentence the customer actually said.
 */
const BODY_INK: Partial<Record<Voice, string>> = {
  system: 'text-[0.875rem] leading-[1.65] text-ink-2',
  note: 'text-[0.875rem] leading-[1.65] text-ink-2',
}
const BODY_DEFAULT = 'text-[0.9375rem] leading-[1.7] text-ink'

/** How far a turn stands off the one above it. A new question opens a gap. */
function spaceAbove(turn: Turn, previous: Turn | undefined): string {
  if (!previous) return ''
  if (turn.voice === 'user') return 'mt-10'
  if (turn.voice === 'handoff') return 'mt-9'
  if (turn.voice === previous.voice && turn.spanId === previous.spanId) return 'mt-3'
  return 'mt-6'
}

/**
 * The two columns every turn is laid on, declared once.
 *
 * A hanging margin only reads as one if every turn hangs by the same amount. A
 * second copy of this string is how the clock on a handoff drifts a few pixels
 * off the clock on the turn above it, which is exactly the kind of error nobody
 * sees and everybody feels.
 */
const TURN_GRID =
  'grid grid-cols-[3.5rem_minmax(0,1fr)] gap-x-5 sm:grid-cols-[4.5rem_minmax(0,1fr)] sm:gap-x-7'

/**
 * How much of something is still withheld, in the units a reader thinks in.
 *
 * A bare "5591" is a figure nobody converts. Everything else measured on this
 * panel is rounded to a unit, and a withheld length is a measurement like any
 * other.
 */
function withheld(chars: number): string {
  if (chars < 1000) return `${chars} more characters`
  if (chars < 1_000_000) return `${(chars / 1000).toFixed(chars < 10_000 ? 1 : 0)}k more characters`
  return `${(chars / 1_000_000).toFixed(1)}M more characters`
}

/** How far into the run this turn is, in the units the inspector uses. */
function offsetLabel(ms: number | null): string {
  if (ms == null) return ''
  if (ms <= 0) return 'start'
  return `+${duration(ms * 1e6)}`
}

/* ---------------------------------------------------------------- turn --- */

function Block({
  label,
  body,
  expanded,
  onExpand,
}: {
  label?: string
  body: string
  expanded: boolean
  onExpand: () => void
}) {
  const long = body.length > LONG
  const shown = long && !expanded ? body.slice(0, LONG) : body
  return (
    <div className="mt-2">
      {label && <p className="legend mb-1 text-ink-3">{label}</p>}
      {/* A recess is where this system puts machine data, and the scroller
          keeps a long result from pushing the next turn off the screen. Nothing
          is hidden by the cap: the height scrolls, and the length that is not
          rendered at all is stated under it. */}
      <div className="well data max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-[2px] p-2.5 text-[0.75rem] leading-relaxed text-ink">
        {shown}
      </div>
      {long && !expanded && (
        <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1.5">
          <span className="text-[0.6875rem] text-ink-3">
            {withheld(body.length - LONG)} are not rendered.
          </span>
          <Action onClick={onExpand} title="Render the rest of this">
            show all
          </Action>
        </div>
      )}
    </div>
  )
}

function TurnView({
  turn,
  previous,
  onOpenStep,
}: {
  turn: Turn
  previous: Turn | undefined
  onOpenStep: (spanId: string) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const expand = useCallback(() => setExpanded(true), [])

  const text = turn.text ?? ''
  const longText = text.length > LONG
  const shownText = longText && !expanded ? text.slice(0, LONG) : text

  if (turn.voice === 'handoff') {
    return (
      <article className={`${spaceAbove(turn, previous)} ${TURN_GRID}`}>
        <span className="data pt-[0.15rem] text-right text-[0.6875rem] text-ink-3">
          {offsetLabel(turn.offsetMs)}
        </span>
        <p className="min-w-0 text-[0.875rem] text-ink-2">
          {turn.text}
          <button
            type="button"
            onClick={() => onOpenStep(turn.spanId)}
            title={`Read ${turn.spanName} as a step`}
            className="data ml-3 text-[0.6875rem] text-ink-3 transition-colors hover:text-amber"
          >
            {turn.spanName}
          </button>
        </p>
      </article>
    )
  }

  return (
    <article
      className={`${spaceAbove(turn, previous)} ${TURN_GRID}`}
    >
      {/* The clock hangs in the margin in the measurement face, right-aligned so
          the figures line up on their units rather than on their first digit. */}
      <span
        className="data pt-[0.2rem] text-right text-[0.6875rem] text-ink-3"
        title={`This step started ${offsetLabel(turn.offsetMs)} into the run`}
      >
        {offsetLabel(turn.offsetMs)}
      </span>

      <div className="min-w-0">
        <p className="flex flex-wrap items-baseline gap-x-2.5 gap-y-0.5">
          <span className={`display text-[0.9375rem] ${VOICE_INK[turn.voice]}`}>{turn.speaker}</span>
          {/* The step's own name is already a control on this line, so an
              attribution that repeats it would print the same word twice. */}
          {turn.attribution && turn.attribution !== turn.spanName && (
            <span className="data text-[0.6875rem] text-ink-3">{turn.attribution}</span>
          )}
          {turn.running && <span className="data text-[0.6875rem] text-amber">running</span>}
          <button
            type="button"
            onClick={() => onOpenStep(turn.spanId)}
            title={`Read ${turn.spanName} as a step`}
            className="data text-[0.6875rem] text-ink-3 transition-colors hover:text-amber"
          >
            {turn.spanName}
          </button>
        </p>

        {text && (
          <p
            className={`mt-1.5 max-w-[62ch] whitespace-pre-wrap break-words ${
              BODY_INK[turn.voice] ?? BODY_DEFAULT
            }`}
          >
            {shownText}
          </p>
        )}
        {longText && !expanded && (
          <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1.5">
            <span className="text-[0.6875rem] text-ink-3">
              {withheld(text.length - LONG)} are not rendered.
            </span>
            <Action onClick={expand} title="Render the rest of this turn">
              show all
            </Action>
          </div>
        )}

        {turn.data && (
          <Block label={turn.dataLabel} body={turn.data} expanded={expanded} onExpand={expand} />
        )}
        {turn.result && (
          <Block label={turn.resultLabel} body={turn.result} expanded={expanded} onExpand={expand} />
        )}

        {turn.error && (
          <p className="mt-2 max-w-[62ch] text-[0.875rem] leading-relaxed text-oxide">
            {turn.error.message}
            {turn.error.kind && (
              <span className="data ml-2 text-[0.6875rem] text-ink-3">{turn.error.kind}</span>
            )}
          </p>
        )}

        {/* The transcript talking about itself: why this turn reads the way it
            does, or what the tree could not say. Set quietly and in the reading
            face, because it is not part of what was said. */}
        {turn.note && (
          <p className="mt-1.5 max-w-[58ch] text-[0.8125rem] leading-relaxed text-ink-3">
            {turn.note}
          </p>
        )}
      </div>
    </article>
  )
}

/* -------------------------------------------------------------- reading --- */

function Provenance({ conversation }: { conversation: Conversation }) {
  const { used, total, skipped } = conversation
  const names = skipped.slice(0, 3).map((s) => s.name)
  return (
    <p className="max-w-[70ch] text-[0.8125rem] leading-relaxed text-ink-2">
      Read back from {used} of {total} step{total === 1 ? '' : 's'}. mcpd stores no conversation:
      this is each step's own prompt and completion, in the order they ran.
      {skipped.length > 0 && (
        <>
          {' '}
          {skipped.length} step{skipped.length === 1 ? '' : 's'} carr
          {skipped.length === 1 ? 'ies' : 'y'} no speech and {skipped.length === 1 ? 'is' : 'are'}{' '}
          not shown ({names.join(', ')}
          {skipped.length > names.length ? `, and ${skipped.length - names.length} more` : ''}).
          Read them as steps.
        </>
      )}
    </p>
  )
}

function Transcript({
  detail,
  onOpenStep,
}: {
  detail: TraceDetail
  onOpenStep: (spanId: string) => void
}) {
  const conversation = useMemo(() => readConversation(detail), [detail])

  return (
    <div className="min-h-0 overflow-y-auto px-5 py-7 sm:px-8">
      <div className="mx-auto max-w-[48rem]">
        <Provenance conversation={conversation} />

        {conversation.empty ? (
          <p className="mt-6 max-w-[62ch] text-[0.9375rem] leading-relaxed text-ink-2">
            No step in this run carries a prompt or a completion, so there is nothing to read back
            as a conversation. mcpd stores what the producer sent it, and this run sent none. Read
            it as steps instead.
          </p>
        ) : (
          <div className="mt-8">
            {conversation.turns.map((t, i) => (
              <TurnView
                key={t.key}
                turn={t}
                previous={conversation.turns[i - 1]}
                onOpenStep={onOpenStep}
              />
            ))}
          </div>
        )}

        {/* The end of a transcript should look like an end rather than like the
            page running out. */}
        {!conversation.empty && (
          <p className="mt-12 border-t border-scribe-2 pt-4 text-[0.8125rem] text-ink-3">
            End of the run.{' '}
            {detail.trace.is_complete
              ? `It ran for ${detail.trace.duration_ns != null ? duration(detail.trace.duration_ns) : 'an unrecorded time'}.`
              : 'It has not ended: more may still arrive.'}
          </p>
        )}
      </div>
    </div>
  )
}

/* ----------------------------------------------------------------- list --- */

function RunList({
  traces,
  loading,
  error,
  selected,
  onSelect,
  filtered,
}: {
  traces: Trace[]
  loading: boolean
  error: string | null
  selected: string | null
  onSelect: (id: string) => void
  filtered: boolean
}) {
  if (error) {
    return (
      <div className="border-b border-scribe-2 p-5 lg:border-b-0 lg:border-r">
        <p className="legend text-oxide">Query failed</p>
        <p className="mt-2 text-[0.875rem] text-ink-2">{error}</p>
      </div>
    )
  }

  if (!traces.length) {
    return (
      <div className="border-b border-scribe-2 p-5 lg:border-b-0 lg:border-r">
        <p className="legend text-ink-3">{loading ? 'Reading' : 'Nothing here'}</p>
        <p className="mt-2 max-w-[34ch] text-[0.875rem] leading-relaxed text-ink-2">
          {loading
            ? 'Asking the daemon.'
            : filtered
              ? 'No run matches that search. Clear it to see the whole project.'
              : 'This project has no runs yet. Send one and it will appear here.'}
        </p>
      </div>
    )
  }

  return (
    <ol
      className="min-h-0 divide-y divide-scribe-2 overflow-y-auto border-b border-scribe-2 lg:border-b-0 lg:border-r"
      role="listbox"
      aria-label="Runs"
    >
      {traces.map((t) => {
        const failed = t.status === 'error'
        const on = selected === t.trace_id
        return (
          <li key={t.trace_id}>
            <button
              type="button"
              role="option"
              aria-selected={on}
              onClick={() => onSelect(t.trace_id)}
              className={`w-full px-4 py-3 text-left transition-colors ${
                on ? 'bg-panel-3' : 'hover:bg-panel-2'
              }`}
            >
              <div className="flex items-baseline gap-2">
                <span className={`truncate text-[0.875rem] ${failed ? 'text-oxide' : 'text-ink'}`}>
                  {t.name}
                </span>
                {failed && <span className="data shrink-0 text-[0.625rem] text-oxide">fault</span>}
                <span
                  className="data ml-auto shrink-0 text-[0.6875rem] text-ink-3"
                  title={t.start_time}
                >
                  {ago(t.start_time)}
                </span>
              </div>
              <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-1">
                {/* What this screen is about: how much of the run is speech. */}
                <span className="data text-[0.6875rem] text-ink-3">
                  {t.model_count} model{t.model_count === 1 ? '' : 's'}
                </span>
                <span className="data text-[0.6875rem] text-ink-3">
                  {t.tool_count} tool{t.tool_count === 1 ? '' : 's'}
                </span>
                {t.correlation.thread_id && (
                  <span
                    className="data max-w-[12rem] truncate text-[0.6875rem] text-ink-3"
                    title={`Thread ${t.correlation.thread_id}`}
                  >
                    {t.correlation.thread_id}
                  </span>
                )}
                {!t.is_complete && <span className="data text-[0.6875rem] text-amber">running</span>}
              </div>
            </button>
          </li>
        )
      })}
    </ol>
  )
}

/* --------------------------------------------------------------- screen --- */

export default function Messages({
  endpoint,
  onEndpoint,
  project,
  onProject,
  initialTrace,
  onOpenThread,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
  initialTrace?: string
  /** Open this thread's own list of runs, on the thread screen. */
  onOpenThread: (threadId: string, traceId?: string) => void
}) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const caps = useCapabilities(endpoint)

  const [query, setQuery] = useState('')
  const [traces, setTraces] = useState<Trace[]>([])
  const [loading, setLoading] = useState(false)
  const [listError, setListError] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(initialTrace ?? null)
  const [detail, setDetail] = useState<TraceDetail | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [reading, setReading] = useState<'conversation' | 'steps'>('conversation')
  const [openSpan, setOpenSpan] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  const debounced = useDebounced(query, 250)

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (conn.state !== 'connected') return
      setLoading(true)
      setListError(null)
      try {
        const params: TraceQuery = {
          project,
          q: debounced || undefined,
          sort: 'start_time',
          order: 'desc',
          limit: PAGE,
        }
        const res = await api.traces(params, { endpoint, signal })
        setTraces(res.traces)
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setListError(e instanceof Error ? e.message : String(e))
      } finally {
        setLoading(false)
      }
    },
    [conn.state, project, debounced, endpoint],
  )

  useEffect(() => {
    const ac = new AbortController()
    void load(ac.signal)
    return () => ac.abort()
  }, [load])

  // Open on the newest run rather than on an empty pane, the way the explorer
  // does: the first thing anyone does here is read the top one.
  useEffect(() => {
    if (!selected && traces.length) setSelected(traces[0].trace_id)
  }, [traces, selected])

  const readDetail = useCallback(
    async (signal?: AbortSignal) => {
      if (!selected || conn.state !== 'connected') return
      try {
        const d = await api.trace(
          selected,
          { project, include: 'all' },
          { endpoint, signal },
        )
        setDetail(d)
        setDetailError(null)
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setDetail(null)
        setDetailError(e instanceof Error ? e.message : String(e))
      }
    },
    [selected, project, endpoint, conn.state],
  )

  useEffect(() => {
    if (!selected) {
      setDetail(null)
      return
    }
    const ac = new AbortController()
    setDetail(null)
    setOpenSpan(null)
    void readDetail(ac.signal)
    return () => ac.abort()
  }, [selected, readDetail])

  const openStep = useCallback((spanId: string) => {
    setOpenSpan(spanId)
    setReading('steps')
  }, [])

  if (conn.state !== 'connected') {
    return (
      <Disconnected
        conn={conn}
        endpoint={endpoint}
        onEndpoint={onEndpoint}
        onRetry={retry}
      />
    )
  }

  const trace = detail?.trace
  const thread = trace?.correlation.thread_id
  const searchDead = caps?.text_search === 'none'

  /* One set of controls, drawn in whichever header is on screen, so switching
     reading does not change what the run offers. */
  const actions = (
    <>
      {thread ? (
        <Action
          onClick={() => onOpenThread(thread, trace?.trace_id)}
          title={`Read the other runs in thread ${thread}`}
        >
          thread
        </Action>
      ) : (
        /* Held rather than dropped, so the control row does not change shape
           between one run and the next, and refused honestly: the reason a
           thread cannot be opened is a fact about the run, not a missing
           feature. */
        <Action
          onClick={() => {}}
          disabled
          title="No step in this run carries a thread id, so mcpd cannot group it with any other run."
        >
          no thread
        </Action>
      )}
      <Action
        onClick={() => {
          setRefreshing(true)
          void readDetail().finally(() => setRefreshing(false))
        }}
        disabled={refreshing}
        title="Read this run again"
      >
        {refreshing ? 'reading' : 'refresh'}
      </Action>
      {/* Two readings of one run. Not a tab strip: the same raised control the
          panel uses everywhere, pressed to show which reading is on screen. */}
      <Action
        onClick={() => setReading('conversation')}
        pressed={reading === 'conversation'}
        title="Read this run as what was said"
      >
        conversation
      </Action>
      <Action
        onClick={() => setReading('steps')}
        pressed={reading === 'steps'}
        title="Read this run as its steps"
      >
        steps
      </Action>
    </>
  )

  return (
    <div className="grid min-h-[calc(100vh-8rem)] grid-rows-[auto_minmax(0,1fr)]">
      <div className="border-b border-scribe bg-panel-1">
        <div className="flex flex-wrap items-center gap-2.5 px-4 py-2.5">
          <DaemonTile health={conn.health} endpoint={endpoint} />
          <ProjectPicker projects={projects} project={project} onProject={onProject} />
          <SearchField
            className="w-full max-w-[24rem] lg:w-[24rem]"
            value={query}
            onChange={setQuery}
            label="Search runs"
            placeholder="search prompts and completions"
          />
          {searchDead && caps && (
            <span className="max-w-[38ch] text-[0.75rem] leading-snug text-ink-2">
              {capabilityNote(caps.backend, 'text_search')}
            </span>
          )}
          <span className="data text-[0.75rem] text-ink-3 lg:ml-auto">
            {loading ? 'reading…' : `${traces.length} run${traces.length === 1 ? '' : 's'}`}
          </span>
        </div>
        <div className="ticks opacity-40" aria-hidden="true" />
      </div>

      <div className="grid min-h-0 lg:grid-cols-[minmax(0,23rem)_minmax(0,1fr)]">
        <RunList
          traces={traces}
          loading={loading}
          error={listError}
          selected={selected}
          onSelect={setSelected}
          filtered={Boolean(debounced)}
        />

        <section className="grid min-h-0 overflow-hidden">
          {!trace || !detail ? (
            <div className="grid place-content-center p-10">
              <p className="max-w-[42ch] text-center text-[0.875rem] leading-relaxed text-ink-2">
                {detailError ??
                  (selected ? 'Reading run…' : 'Choose a run to read it as a conversation.')}
              </p>
            </div>
          ) : reading === 'steps' ? (
            // One header, not two: the panel already draws the run's name and
            // its readings, so this screen hands it the controls rather than
            // stacking a second header of its own above it. The panel reads the
            // detail this screen already holds, so moving between the two
            // readings costs no second request.
            <TracePanel
              traceId={trace.trace_id}
              project={project}
              endpoint={endpoint}
              detail={detail}
              spanId={openSpan}
              onSpanChange={setOpenSpan}
              className="min-h-0"
              headerAction={actions}
            />
          ) : (
            <div className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
              <header className="border-b border-scribe-2 px-5 py-3.5">
                <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1.5">
                  <h2 className="display text-[1.125rem]">{trace.name}</h2>
                  <CopyValue value={trace.trace_id} className="data text-[0.6875rem] text-ink-3">
                    <span>{trace.trace_id}</span>
                  </CopyValue>
                  {trace.status === 'error' && (
                    <span className="annunciator py-1 text-[0.625rem]" data-lit="oxide">
                      Fault
                    </span>
                  )}
                  <div className="ml-auto flex flex-wrap items-center gap-1.5">{actions}</div>
                </div>
                {/* The same readings, in the same order, the panel puts under
                    its own header, so switching reading does not move them. */}
                <dl className="mt-2 flex flex-wrap gap-x-6 gap-y-1">
                  <Reading label="Duration">
                    {trace.duration_ns != null ? duration(trace.duration_ns) : 'not ended'}
                  </Reading>
                  <Reading label="Steps">{String(trace.span_count)}</Reading>
                  <Reading label="Models">{String(trace.model_count)}</Reading>
                  <Reading label="Tools">{String(trace.tool_count)}</Reading>
                  <Reading label="Tokens">
                    {trace.usage?.total_tokens != null
                      ? tokens(trace.usage.total_tokens)
                      : trace.usage?.input_tokens != null && trace.usage.output_tokens != null
                        ? tokens(trace.usage.input_tokens + trace.usage.output_tokens)
                        : 'none reported'}
                  </Reading>
                  <Reading label="Cost">
                    {trace.cost?.total_cost != null
                      ? money(trace.cost.total_cost, trace.cost.currency)
                      : 'not priced'}
                  </Reading>
                </dl>
              </header>

              <Transcript key={trace.trace_id} detail={detail} onOpenStep={openStep} />
            </div>
          )}
        </section>
      </div>
    </div>
  )
}

function Reading({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <dt className="legend text-ink-3">{label}</dt>
      <dd className="data text-[0.8125rem] text-ink">{children}</dd>
    </div>
  )
}
