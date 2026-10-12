/**
 * Everything one step carries.
 *
 * The order is the order a reader asks for it: what it was and how long it
 * took, then what it spent, then why it failed, then what went in and came out,
 * and last the ids that tie it to the rest of your system. Prev and next walk
 * the same rows the waterfall drew, so moving through the panel with the
 * keyboard visits the steps in the order they are on screen.
 */

import { useMemo } from 'react'
import type { LogRecord, Span, TraceDetail } from '@/generated/types'
import { costBlank, duration, money } from '@/lib/api'
import { Payload } from './Payload'
import { TokenUsageView } from './TokenUsage'
import { kindColour, kindLabel, selfTimeNs, stopReason } from './trace-tree'
import type { Row } from './trace-tree'
import { Action, CopyValue, Highlight, PAYLOAD_TINT } from './trace-ui'

/** One label and one reading. `muted` marks a reason standing where a figure would. */
interface Fact {
  label: string
  value: string
  muted?: boolean
  title?: string
}

function Facts({ facts, query }: { facts: Fact[]; query: string }) {
  return (
    <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1.5">
      {facts.map((f) => (
        <div key={f.label} className="min-w-0">
          <dt className="legend text-ink-3">{f.label}</dt>
          {/* A figure is measurement and sits in the measurement face. A reason
              is prose, and wearing the same face would dress it as a reading. */}
          <dd
            className={`break-words ${
              f.muted ? 'text-[0.8125rem] text-ink-3' : 'data text-[0.8125rem] text-ink'
            }`}
            title={f.title}
          >
            <Highlight text={f.value} query={query} />
          </dd>
        </div>
      ))}
    </dl>
  )
}

/** A clock reading, to the millisecond, because a waterfall is read that way. */
function stamp(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const t = d.toLocaleTimeString(undefined, { hour12: false })
  return `${t}.${String(d.getMilliseconds()).padStart(3, '0')}`
}

function spanFacts(span: Span, spans: Span[], t0: number): Fact[] {
  const self = selfTimeNs(span, spans)
  const blank = costBlank(span)
  const started = new Date(span.start_time).getTime()
  const offset = Number.isNaN(started) ? null : started - t0
  const facts: (Fact | null)[] = [
    { label: 'Kind', value: kindLabel(span.kind) },
    { label: 'Status', value: span.status },
    {
      label: 'Duration',
      value:
        span.duration_ns != null
          ? duration(span.duration_ns)
          : span.is_event
            ? 'an event'
            : 'not ended',
      muted: span.duration_ns == null,
      title:
        span.duration_ns != null
          ? undefined
          : span.is_event
            ? 'An event marks a point in time, so it has no duration.'
            : 'This step has not ended.',
    },
    self != null
      ? {
          label: 'Self',
          value: duration(self),
          title: 'Time inside this step that none of its children accounts for.',
        }
      : null,
    { label: 'Started', value: stamp(span.start_time) },
    span.end_time ? { label: 'Ended', value: stamp(span.end_time) } : null,
    offset != null
      ? {
          label: 'Offset',
          value: offset === 0 ? '0' : `+${duration(offset * 1e6)}`,
          title: 'How far into the run this step started.',
        }
      : null,
    span.model ? { label: 'Model', value: span.model } : null,
    span.provider ? { label: 'Provider', value: span.provider } : null,
    span.entity_name ? { label: 'Entity', value: span.entity_name } : null,
    span.cost?.total_cost != null
      ? { label: 'Cost', value: money(span.cost.total_cost, span.cost.currency) }
      : blank
        ? { label: 'Cost', value: blank.short, muted: true, title: blank.long }
        : null,
  ]
  return facts.filter((f): f is Fact => f !== null)
}

/**
 * The ids this step carries, and only those.
 *
 * A row is drawn for a value the span actually has. Printing a dash against
 * every field mcpd can model turns eleven absent ids into eleven rows of noise
 * and buries the two that are set.
 */
function correlationRows(span: Span): { label: string; value: string; note?: string }[] {
  const c = span.correlation
  const rows: { label: string; value?: string; note?: string }[] = [
    { label: 'Trace', value: span.trace_id },
    { label: 'Span', value: span.span_id },
    { label: 'Parent span', value: span.parent_span_id },
    { label: 'External parent', value: span.external_parent_span_id, note: 'From a tracing system outside mcpd, carried for correlation only.' },
    { label: 'Project', value: c?.project },
    { label: 'Environment', value: c?.environment },
    { label: 'Service', value: c?.service_name },
    { label: 'Service version', value: c?.service_version },
    { label: 'Release', value: c?.release },
    { label: 'Run', value: c?.run_id, note: "The producer's own identifier for this run." },
    { label: 'Thread', value: c?.thread_id },
    { label: 'Session', value: c?.session_id },
    { label: 'Request', value: c?.request_id },
    {
      label: 'Resource',
      value: c?.resource_id,
      note: 'The tenant or workspace inside the calling application. mcpd models one organisation-level id, and this is it.',
    },
    {
      label: 'User',
      value: c?.user_id,
      note: 'Hashed with a keyed digest at ingest, so this is the grouping key rather than who the customer is.',
    },
    { label: 'Experiment', value: c?.experiment_id },
  ]
  return rows.filter((r): r is { label: string; value: string; note?: string } => !!r.value)
}

export function SpanInspector({
  span,
  detail,
  rows,
  onSelect,
  query,
  payloadOnly,
  t0,
  className = '',
  id,
}: {
  span: Span | null
  detail: TraceDetail
  /** The rows the waterfall is showing, in order, so prev and next agree with it. */
  rows: Row[]
  onSelect: (id: string) => void
  /** The panel's search, painted into everything this shows. */
  query: string
  /** True when this step was kept by the search on its payload alone. */
  payloadOnly: boolean
  /** The run's first start, so an offset can be measured. */
  t0: number
  className?: string
  id: string
}) {
  const logs = useMemo(
    () => (detail.logs ?? []).filter((l: LogRecord) => span && l.span_id === span.span_id),
    [detail.logs, span],
  )

  const at = span ? rows.findIndex((r) => r.span.span_id === span.span_id) : -1
  const prev = at > 0 ? rows[at - 1] : null
  const next = at >= 0 && at < rows.length - 1 ? rows[at + 1] : null

  if (!span) {
    return (
      <aside
        id={id}
        aria-label="Run detail"
        className={`min-h-0 overflow-y-auto border-t border-scribe-2 p-5 ${className}`}
      >
        <p className="legend text-ink-3">Run detail</p>
        {(detail.scores?.length ?? 0) > 0 && (
          <div className="mt-4">
            {detail.scores!.map((s) => (
              <div key={s.score_id} className="mb-3">
                <div className="flex items-baseline gap-2">
                  <span className="legend text-ink-2">{s.scorer_id}</span>
                  {/* Not coloured against a threshold. A score carries no
                      direction in this data, so a rule that reads low as bad is
                      inverted for every scorer where high is the bad end. */}
                  <span className="data text-[0.875rem] text-ink">{s.value.toFixed(2)}</span>
                  <span className="data text-[0.625rem] text-ink-3">
                    {s.min != null && s.min !== 0 ? `${s.min} to ${s.max ?? 1}` : `of ${s.max ?? 1}`}
                  </span>
                </div>
                {s.reason && (
                  <p className="mt-1 text-[0.8125rem] leading-snug text-ink-2">{s.reason}</p>
                )}
              </div>
            ))}
          </div>
        )}
        {(detail.feedback?.length ?? 0) > 0 && (
          <div className="mt-5 border-t border-scribe-2 pt-4">
            <p className="legend text-ink-3">Human feedback</p>
            {detail.feedback!.map((f) => (
              <div key={f.feedback_id} className="mt-2">
                <span className="data text-[0.8125rem] text-ink">
                  {f.kind}: {JSON.stringify(f.value)}
                </span>
                {f.comment && <p className="mt-1 text-[0.8125rem] text-ink-2">{f.comment}</p>}
              </div>
            ))}
          </div>
        )}
        <p className="mt-5 border-t border-scribe-2 pt-4 text-[0.8125rem] leading-relaxed text-ink-3">
          Choose a step to read its prompt, its completion, what it spent and the ids it carries.
        </p>
      </aside>
    )
  }

  const blank = costBlank(span)
  const stop = stopReason(span)
  const tone = kindColour(span.kind)
  const ids = correlationRows(span)

  return (
    <aside
      // Keyed by the step, so moving to another step settles the detail in
      // again and starts it from its top rather than mid-scroll.
      key={span.span_id}
      id={id}
      aria-label="Step detail"
      className={`app-enter min-h-0 overflow-y-auto border-t border-scribe-2 p-5 ${className}`}
    >
      <div className="flex items-center gap-2">
        <Action onClick={() => prev && onSelect(prev.span.span_id)} disabled={!prev} title={prev ? `Previous step: ${prev.span.name}` : 'This is the first step shown'}>
          prev
        </Action>
        <Action onClick={() => next && onSelect(next.span.span_id)} disabled={!next} title={next ? `Next step: ${next.span.name}` : 'This is the last step shown'}>
          next
        </Action>
        {at >= 0 && (
          <span className="data text-[0.6875rem] text-ink-3">
            step {at + 1} of {rows.length}
          </span>
        )}
      </div>

      <h3 className="display mt-3 text-[1.0625rem]">
        <Highlight text={span.name} query={query} />
      </h3>
      {/* The same costume the kind wears in the waterfall, so one datum does
          not change face between the row you clicked and the panel it opened. */}
      <p className={`data mt-1 text-[0.625rem] uppercase tracking-wider ${tone.text}`}>
        {kindLabel(span.kind)}
      </p>

      {payloadOnly && (
        <p
          className="mt-2 inline-block rounded-[1px] px-1.5 py-1 text-[0.75rem] leading-snug text-ink-2"
          style={{ background: PAYLOAD_TINT }}
        >
          Your search matches this step in its details rather than in its name.
        </p>
      )}

      <Facts facts={spanFacts(span, detail.spans, t0)} query={query} />

      {/* The whole reason, under the readings. A step that spends no tokens is
          left to its short form: the kind is already on the panel and the
          sentence would repeat it on every tool call in the run. */}
      {blank && blank.code !== 'not-a-model-call' && (
        <p className="mt-2 max-w-[52ch] text-[0.8125rem] leading-snug text-ink-2">{blank.long}</p>
      )}

      {span.error && (
        <div className="well mt-4 rounded-[2px] p-3">
          <p className="legend text-oxide">Error</p>
          <p className="mt-1 text-[0.8125rem] leading-snug text-ink">
            <Highlight text={span.error.message} query={query} />
          </p>
          {span.error.kind && (
            <p className="data mt-1 text-[0.6875rem] text-ink-3">{span.error.kind}</p>
          )}
          {span.error.stack && (
            <pre className="scroll-x data mt-2 max-h-40 overflow-auto text-[0.6875rem] leading-relaxed text-ink-2">
              {span.error.stack}
            </pre>
          )}
        </div>
      )}

      <TokenUsageView usage={span.usage} stop={stop} />

      {stop && !stop.atLimit && (
        <p className="mt-2 text-[0.75rem] leading-snug text-ink-3">
          Stopped on <span className="data text-ink-2">{stop.key}</span>{' '}
          <span className="data text-ink-2">{stop.value}</span>.
        </p>
      )}

      {(span.tags?.length ?? 0) > 0 && (
        <div className="mt-5">
          <h4 className="legend text-ink-3">Tags</h4>
          <p className="data mt-1 text-[0.8125rem] leading-relaxed text-ink">
            <Highlight text={span.tags!.join('  ·  ')} query={query} />
          </p>
        </div>
      )}

      <Payload label="Input" value={span.input} traceQuery={query} spanId={span.span_id} />
      <Payload label="Output" value={span.output} traceQuery={query} spanId={span.span_id} />
      <Payload label="Attributes" value={span.attributes} traceQuery={query} spanId={span.span_id} />
      <Payload label="Metadata" value={span.metadata} traceQuery={query} spanId={span.span_id} />

      {logs.length > 0 && (
        <div className="mt-5">
          <h4 className="legend text-ink-3">Logs</h4>
          <ul className="mt-2 space-y-1">
            {logs.map((l) => (
              <li key={l.log_id} className="data text-[0.75rem] leading-snug">
                <span
                  className={
                    l.level === 'error' || l.level === 'fatal'
                      ? 'text-oxide'
                      : l.level === 'warn'
                        ? 'text-amber'
                        : 'text-ink-3'
                  }
                >
                  {l.level}
                </span>{' '}
                <span className="text-ink-2">
                  <Highlight text={l.message} query={query} />
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="mt-5 border-t border-scribe-2 pt-4">
        <h4 className="legend text-ink-3">Identity</h4>
        <p className="mt-1 text-[0.75rem] leading-snug text-ink-3">
          Press a value to copy it.
        </p>
        {/* One grid, so the labels and the values each hold a single column
            down the whole block whatever length any one id happens to be. */}
        <dl className="mt-2 grid grid-cols-[7.5rem_minmax(0,1fr)] items-baseline gap-x-3 gap-y-1.5">
          {ids.map((r) => (
            <IdRow key={r.label} label={r.label} value={r.value} note={r.note} query={query} />
          ))}
        </dl>
      </div>
    </aside>
  )
}

function IdRow({
  label,
  value,
  note,
  query,
}: {
  label: string
  value: string
  note?: string
  query: string
}) {
  return (
    <>
      <dt className="legend text-[0.5625rem] text-ink-3" title={note}>
        {label}
      </dt>
      <dd className="min-w-0">
        <CopyValue value={value} className="data block truncate text-[0.75rem] text-ink">
          <span className="block truncate" title={value}>
            <Highlight text={value} query={query} />
          </span>
        </CopyValue>
      </dd>
    </>
  )
}
