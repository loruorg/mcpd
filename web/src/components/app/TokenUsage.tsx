/**
 * What a model call spent, drawn rather than listed.
 *
 * Two counts and six categories are readable as figures, but the question a
 * reader actually has is a proportion: how much of this call was the prompt,
 * and how much of the prompt was cache. So the split is a bar and each category
 * carries its share of its own side, all of it on one grid so the shares can be
 * compared down the column rather than one row at a time.
 */

import { Fragment } from 'react'
import type { TokenUsage } from '@/generated/types'
import { tokens } from '@/lib/api'
import type { StopReason } from './trace-tree'

/**
 * The two tones the split is drawn in.
 *
 * One hue in two values rather than two hues: input and output are two parts of
 * one measurement, and giving them unrelated colours would say they are two
 * different things. Neither tone is a signal colour, because a token count is
 * not a fault, an attention or a nominal reading.
 */
const IN_TONE = 'var(--color-slate)'
const OUT_TONE = 'color-mix(in oklab, var(--color-slate) 42%, var(--color-panel-3))'

interface Category {
  label: string
  value: number
}

function inputCategories(u: TokenUsage): Category[] {
  const d = u.input_details
  if (!d) return []
  return [
    { label: 'text', value: d.text },
    { label: 'cache read', value: d.cache_read },
    { label: 'cache write', value: d.cache_write },
    { label: 'cache write 5m', value: d.cache_write_5m },
    { label: 'cache write 1h', value: d.cache_write_1h },
    { label: 'audio', value: d.audio },
    { label: 'image', value: d.image },
  ].filter((c): c is Category => c.value != null)
}

function outputCategories(u: TokenUsage): Category[] {
  const d = u.output_details
  if (!d) return []
  return [
    { label: 'text', value: d.text },
    { label: 'reasoning', value: d.reasoning },
    { label: 'audio', value: d.audio },
    { label: 'image', value: d.image },
  ].filter((c): c is Category => c.value != null)
}

export function TokenUsageView({ usage, stop }: { usage?: TokenUsage; stop: StopReason | null }) {
  if (!usage) return null
  const inTok = usage.input_tokens
  const outTok = usage.output_tokens
  const total = usage.total_tokens ?? (inTok != null && outTok != null ? inTok + outTok : null)
  const ins = inputCategories(usage)
  const outs = outputCategories(usage)
  if (total == null && inTok == null && outTok == null && !ins.length && !outs.length) return null

  const split = inTok != null && outTok != null && inTok + outTok > 0
  const denom = split ? inTok + outTok : 0

  return (
    <section className="mt-5 border-t border-scribe-2 pt-4">
      <div className="flex items-baseline gap-3">
        <h4 className="legend text-ink-3">Tokens</h4>
        <span className="data text-[0.9375rem] text-ink">
          {total != null ? tokens(total) : inTok != null ? tokens(inTok) : tokens(outTok ?? 0)}
        </span>
        <span className="text-[0.6875rem] text-ink-3">{total != null ? 'total' : 'reported'}</span>
      </div>

      {/* The bar fills its track exactly when both halves are known, and is not
          drawn at all when they are not: a half-filled track against no
          denominator invites the reader to measure a proportion nobody
          reported. */}
      {split ? (
        <>
          <div className="mt-2.5 flex h-2 w-full overflow-hidden rounded-[1px]">
            <div
              style={{ width: `${(inTok / denom) * 100}%`, background: IN_TONE }}
              title={`Input ${inTok.toLocaleString()} tokens`}
            />
            <div
              style={{ width: `${(outTok / denom) * 100}%`, background: OUT_TONE }}
              title={`Output ${outTok.toLocaleString()} tokens`}
            />
          </div>
          <div className="mt-1.5 flex flex-wrap items-baseline gap-x-5 gap-y-1">
            {/* The output percentage is the remainder rather than its own
                rounding, so the two never add to 101. */}
            <Key tone={IN_TONE} label="input" value={inTok} percent={Math.round((inTok / denom) * 100)} />
            <Key
              tone={OUT_TONE}
              label="output"
              value={outTok}
              percent={100 - Math.round((inTok / denom) * 100)}
            />
          </div>
        </>
      ) : (
        <p className="mt-2 max-w-[52ch] text-[0.8125rem] leading-snug text-ink-2">
          {inTok != null
            ? 'The producer reported input tokens and no output tokens, so there is no split to draw.'
            : outTok != null
              ? 'The producer reported output tokens and no input tokens, so there is no split to draw.'
              : 'The producer reported a total with no input and output counts. mcpd prices the two at their own rates and will not split a total it was not given.'}
        </p>
      )}

      {(ins.length > 0 || outs.length > 0) && (
        <div className="mt-3.5 grid gap-x-4 gap-y-3 sm:grid-cols-2">
          <Breakdown title="Input by category" rows={ins} total={inTok} tone={IN_TONE} />
          <Breakdown title="Output by category" rows={outs} total={outTok} tone={OUT_TONE} />
        </div>
      )}

      {stop?.atLimit && (
        <div className="mt-4">
          <span className="annunciator py-1 text-[0.625rem]" data-lit="amber">
            Token limit
          </span>
          <p className="mt-2 max-w-[52ch] text-[0.8125rem] leading-snug text-ink-2">
            The producer reported <span className="data text-ink">{stop.key}</span>{' '}
            <span className="data text-ink">{stop.value}</span>, which means the model stopped
            because it reached its output limit
            {outTok != null ? ` at ${outTok.toLocaleString()} output tokens` : ''}. The completion
            below is cut short, and anything reading it downstream received a partial answer.
          </p>
        </div>
      )}
    </section>
  )
}

function Key({
  tone,
  label,
  value,
  percent,
}: {
  tone: string
  label: string
  value: number
  percent: number
}) {
  return (
    <span className="flex items-baseline gap-1.5">
      {/* The swatch is a rule, in the same tone and the same cap as the bar it
          keys, so it reads as a piece of that bar rather than a bullet. */}
      <span
        className="h-[3px] w-3.5 shrink-0 translate-y-[-2px] rounded-[1.5px]"
        style={{ background: tone }}
        aria-hidden="true"
      />
      <span className="legend text-[0.5625rem] text-ink-3">{label}</span>
      <span className="data text-[0.75rem] text-ink">{tokens(value)}</span>
      <span className="data text-[0.6875rem] text-ink-3">{percent}%</span>
    </span>
  )
}

/**
 * One side's categories.
 *
 * The share is measured against that side's own total where the producer
 * reported one, and against the sum of the categories otherwise, because those
 * two denominators are different claims: providers report categories that do
 * not always add to the total, and quietly renormalising them would hide it.
 */
function Breakdown({
  title,
  rows,
  total,
  tone,
}: {
  title: string
  rows: Category[]
  total?: number
  tone: string
}) {
  if (!rows.length) return null
  const sum = rows.reduce((a, r) => a + r.value, 0)
  const denom = total && total > 0 ? total : sum
  return (
    <div>
      <h5 className="legend text-[0.5625rem] text-ink-3">{title}</h5>
      {/* One grid for every row rather than a grid per row: the count column
          takes one width down the whole list, so the shares line up against
          each other and not against whatever each row happened to need. */}
      <dl className="mt-1.5 grid grid-cols-[minmax(0,1fr)_auto_3.5rem] items-center gap-x-2.5 gap-y-1">
        {rows.map((r) => (
          <Fragment key={r.label}>
            <dt className="truncate text-[0.75rem] text-ink-2">{r.label}</dt>
            <dd className="data text-right text-[0.75rem] text-ink">{tokens(r.value)}</dd>
            <dd>
              <span className="block h-[3px] w-full rounded-[1.5px] bg-scribe-2" aria-hidden="true">
                <span
                  className="block h-full rounded-[1.5px]"
                  style={{
                    width: `${Math.min(100, denom > 0 ? (r.value / denom) * 100 : 0)}%`,
                    background: tone,
                  }}
                />
              </span>
            </dd>
          </Fragment>
        ))}
      </dl>
      {total != null && sum !== total && (
        <p className="mt-1.5 text-[0.6875rem] leading-snug text-ink-3">
          The categories add to {tokens(sum)} against a reported {tokens(total)}. The shares are
          measured against the reported figure.
        </p>
      )}
    </div>
  )
}
