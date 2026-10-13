/**
 * What was written about one run, one line per aspect, in the run's header.
 *
 * A note is the sentence that put this run in each of its themes, so it is the
 * way back from a theme to what happened. A backend without the theme tables
 * answers 501, and that costs this block and nothing else in the panel.
 */

import { useEffect, useState } from 'react'
import type { AspectNote } from '@/generated/types'
import { themesApi } from './themes-api'

export function TraceAspects({
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
  const [notes, setNotes] = useState<AspectNote[] | null>(null)
  const [refused, setRefused] = useState(false)

  useEffect(() => {
    const ac = new AbortController()
    setNotes(null)
    setRefused(false)
    themesApi
      .traceNotes(traceId, project, { endpoint, apiKey, signal: ac.signal })
      .then((r) => setNotes(r.notes))
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setRefused(true)
      })
    return () => ac.abort()
  }, [traceId, project, endpoint, apiKey])

  if (refused || notes === null) return null
  if (notes.length === 0) {
    return (
      <p className="mt-2 text-[0.75rem] text-ink-3">
        No aspect notes about this run yet. A note is written after the run ends.
      </p>
    )
  }
  return (
    <dl className="mt-2.5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1">
      {notes.map((n) => (
        <div key={n.note_id} className="contents">
          <dt className="legend self-baseline text-ink-3">{n.aspect.replace(/_/g, ' ')}</dt>
          <dd
            className="self-baseline text-[0.8125rem] leading-snug text-ink-2"
            title={n.model ? `Written by ${n.model}` : 'Written by the daemon from the run itself'}
          >
            {n.text}
          </dd>
        </div>
      ))}
    </dl>
  )
}
