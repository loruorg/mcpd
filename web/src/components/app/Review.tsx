/**
 * The review queue.
 *
 * Runs a person should look at, queued from the run panel or the API. The
 * queue opens on what is pending; each row opens its run, takes an assignee,
 * and is marked reviewed with what was found, or dismissed. Every change is
 * kept by the daemon as a new version of the item.
 */

import { useEffect, useState } from 'react'
import { request } from '@/lib/api'
import type { ReviewItem, ReviewQueueResponse, ReviewStatus } from '@/generated/types'
import { Disconnected } from './Explorer'
import { useConnection } from './shell'
import { Empty, Failure, message, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

const enc = encodeURIComponent

export default function Review({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [status, setStatus] = useState<ReviewStatus | ''>('pending')
  const [assignee, setAssignee] = useState('')
  const [queue, setQueue] = useState<ReviewQueueResponse | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const p = new URLSearchParams({ project })
    if (status) p.set('status', status)
    if (assignee.trim()) p.set('assignee', assignee.trim())
    request<ReviewQueueResponse>(`/v1/api/review-queue?${p}`, { endpoint, signal: ac.signal })
      .then((q) => {
        setQueue(q)
        setError(null)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [conn.state, endpoint, project, status, assignee, tick])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  const update = (item: ReviewItem, body: Record<string, string>) =>
    request<ReviewItem>(
      `/v1/api/review-queue/${enc(item.review_id)}?project=${enc(project)}`,
      { endpoint },
      body,
      'PATCH',
    )
      .then(() => setTick((t) => t + 1))
      .catch((e) => setError(message(e)))

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={queue ? `${queue.pending} pending` : ''}
      >
        <select
          className="field"
          aria-label="Status"
          value={status}
          onChange={(e) => setStatus(e.target.value as ReviewStatus | '')}
        >
          <option value="pending">pending</option>
          <option value="reviewed">reviewed</option>
          <option value="dismissed">dismissed</option>
          <option value="">every status</option>
        </select>
        <input
          className="field w-36"
          aria-label="Assignee"
          placeholder="anyone"
          value={assignee}
          onChange={(e) => setAssignee(e.target.value)}
        />
      </Toolbar>
      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && <Failure title="Request refused" error={error} />}
        <Section title="Review queue">
          {queue && queue.items.length === 0 ? (
            <Empty>
              Nothing here. Queue a run from its panel with <em>queue for review</em>, or with{' '}
              <code>POST /v1/api/review-queue</code>.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Queued</th>
                    <th className="px-3 py-2">Run</th>
                    <th className="px-3 py-2">Status</th>
                    <th className="px-3 py-2">Assignee</th>
                    <th className="px-3 py-2">Note</th>
                    <th className="px-3 py-2" />
                  </tr>
                </thead>
                <tbody>
                  {queue?.items.map((i) => (
                    <Row key={i.review_id} item={i} onOpenTrace={onOpenTrace} onUpdate={(b) => update(i, b)} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>
      </div>
    </div>
  )
}

function Row({
  item,
  onOpenTrace,
  onUpdate,
}: {
  item: ReviewItem
  onOpenTrace: (id: string) => void
  onUpdate: (body: Record<string, string>) => void
}) {
  const [note, setNote] = useState(item.note ?? '')
  const [who, setWho] = useState(item.assignee ?? '')
  const tone = item.status === 'pending' ? 'text-amber' : item.status === 'reviewed' ? 'text-olive' : 'text-ink-3'
  return (
    <tr className="border-t border-scribe align-top">
      <td className="data px-3 py-1.5 whitespace-nowrap" title={`changed ${when(item.updated_at)}`}>
        {when(item.created_at)}
      </td>
      <td className="px-3 py-1.5">
        <button type="button" className="link data" onClick={() => onOpenTrace(item.trace_id)}>
          {item.trace_id.slice(0, 12)}
        </button>
      </td>
      <td className={`px-3 py-1.5 ${tone}`}>{item.status}</td>
      <td className="px-3 py-1.5">
        <input
          className="field w-28"
          aria-label="Assignee"
          value={who}
          onChange={(e) => setWho(e.target.value)}
          onBlur={() => who !== (item.assignee ?? '') && onUpdate({ assignee: who })}
        />
      </td>
      <td className="px-3 py-1.5">
        <input
          className="field w-full min-w-48"
          aria-label="Note"
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="What you found"
        />
      </td>
      <td className="px-3 py-1.5 whitespace-nowrap">
        <button type="button" className="link" onClick={() => onUpdate({ status: 'reviewed', note })}>
          reviewed
        </button>{' '}
        <button type="button" className="link text-ink-3" onClick={() => onUpdate({ status: 'dismissed', note })}>
          dismiss
        </button>
      </td>
    </tr>
  )
}
