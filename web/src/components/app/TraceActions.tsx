/**
 * What a reader can do to a run from its header: judge it, and keep it as a
 * regression case. Both write through the daemon's own endpoints, so a
 * judgement made here is the same record an SDK's `feedback` call produces,
 * and a case kept here carries the run's id like any other capture.
 */

import { useEffect, useState } from 'react'
import { request } from '@/lib/api'
import type { Dataset, Feedback } from '@/generated/types'
import { Action } from './trace-ui'
import { message, more } from './screen-kit'

function newId(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

export function TraceActions({
  traceId,
  project,
  endpoint,
  apiKey,
}: {
  traceId: string
  project: string
  endpoint: string
  apiKey?: string
}) {
  const [open, setOpen] = useState<'none' | 'judge' | 'keep'>('none')
  const [note, setNote] = useState<string | null>(null)
  const o = { endpoint, apiKey }

  return (
    <>
      <Action
        onClick={() => setOpen(open === 'judge' ? 'none' : 'judge')}
        pressed={open === 'judge'}
        title="Record a person's judgement of this run"
      >
        judge
      </Action>
      <Action
        onClick={() => setOpen(open === 'keep' ? 'none' : 'keep')}
        pressed={open === 'keep'}
        title="Keep this run as a case in a dataset"
      >
        save to dataset
      </Action>
      {note && <span className="text-[0.75rem] text-ink-2">{note}</span>}
      {open === 'judge' && (
        <Judge
          onDone={(n) => {
            setNote(n)
            setOpen('none')
          }}
          send={(kind, value, comment) => {
            const fb: Feedback = {
              feedback_id: newId(),
              trace_id: traceId,
              kind,
              value,
              comment: comment || undefined,
              author_kind: 'reviewer',
              timestamp: new Date().toISOString(),
              correlation: { project },
            }
            return request<Feedback>('/v1/api/feedback', o, fb)
          }}
        />
      )}
      {open === 'keep' && (
        <Keep
          project={project}
          endpoint={endpoint}
          apiKey={apiKey}
          traceId={traceId}
          onDone={(n) => {
            setNote(n)
            setOpen('none')
          }}
        />
      )}
    </>
  )
}

function Judge({
  send,
  onDone,
}: {
  send: (kind: Feedback['kind'], value: unknown, comment: string) => Promise<unknown>
  onDone: (note: string) => void
}) {
  const [comment, setComment] = useState('')
  const [error, setError] = useState<string | null>(null)
  const submit = (up: boolean) => {
    setError(null)
    send('thumbs', up ? 'up' : 'down', comment)
      .then(() => onDone(`judged ${up ? 'good' : 'bad'}`))
      .catch((e) => setError(message(e)))
  }
  return (
    <div className="panel mt-2 flex w-full flex-wrap items-center gap-2 p-2">
      <input
        value={comment}
        onChange={(e) => setComment(e.target.value)}
        placeholder="Why, in a sentence (optional)"
        aria-label="Comment"
        className="field min-w-[16rem] flex-1"
      />
      <Action onClick={() => submit(true)} title="This run did what it should">
        good
      </Action>
      <Action onClick={() => submit(false)} title="This run went wrong">
        bad
      </Action>
      {error && <span className="text-[0.75rem] text-oxide">{error}</span>}
    </div>
  )
}

function Keep({
  project,
  endpoint,
  apiKey,
  traceId,
  onDone,
}: {
  project: string
  endpoint: string
  apiKey?: string
  traceId: string
  onDone: (note: string) => void
}) {
  const [datasets, setDatasets] = useState<Dataset[] | null>(null)
  const [chosen, setChosen] = useState('')
  const [asExpected, setAsExpected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    more
      .datasets(project, { endpoint, apiKey })
      .then((d) => {
        setDatasets(d)
        setChosen(d[0]?.dataset_id ?? '')
      })
      .catch((e) => setError(message(e)))
  }, [project, endpoint, apiKey])
  const keep = () => {
    setError(null)
    more
      .capture(chosen, project, [traceId], asExpected, { endpoint, apiKey })
      .then((r) =>
        onDone(r.captured === 1 ? 'kept as a case' : 'the daemon did not find this run to keep'),
      )
      .catch((e) => setError(message(e)))
  }
  return (
    <div className="panel mt-2 flex w-full flex-wrap items-center gap-2 p-2 text-[0.8125rem]">
      {datasets && datasets.length === 0 ? (
        <span className="text-ink-2">
          This project has no dataset yet. Create one with <code>POST /v1/api/datasets</code>.
        </span>
      ) : (
        <>
          <select value={chosen} onChange={(e) => setChosen(e.target.value)} aria-label="Dataset" className="field">
            {datasets?.map((d) => (
              <option key={d.dataset_id} value={d.dataset_id}>
                {d.name}
              </option>
            ))}
          </select>
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={asExpected} onChange={(e) => setAsExpected(e.target.checked)} />
            its answer is the expected one
          </label>
          <Action onClick={keep} disabled={!chosen} title="Add this run to the dataset">
            keep
          </Action>
        </>
      )}
      {error && <span className="text-oxide">{error}</span>}
    </div>
  )
}
