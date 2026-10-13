/**
 * The prompts screen.
 *
 * Every prompt in the project at its latest version, then one prompt's
 * history: each version's body, what changed from the one before, the labels
 * that point at it, and the steps that ran it. Moving a label is how a prompt
 * change is released or rolled back, so the label controls sit beside the
 * version they move.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { request } from '@/lib/api'
import type { ClientOptions } from '@/lib/api'
import type { DeleteResult, Prompt, PromptVersionsResponse, SpanListResponse } from '@/generated/types'
import { Disconnected } from './Explorer'
import { useConnection } from './shell'
import { Empty, Failure, message, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

const q = (params: Record<string, string | number | undefined>) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') p.set(k, String(v))
  const s = p.toString()
  return s ? `?${s}` : ''
}
const enc = encodeURIComponent

export const prompts = {
  list: (project: string, o?: ClientOptions) => request<Prompt[]>(`/v1/api/prompts${q({ project })}`, o),
  versions: (project: string, name: string, o?: ClientOptions) =>
    request<PromptVersionsResponse>(`/v1/api/prompts/${enc(name)}/versions${q({ project })}`, o),
  create: (project: string, body: Record<string, unknown>, o?: ClientOptions) =>
    request<Prompt>(`/v1/api/prompts${q({ project })}`, o, body),
  label: (project: string, name: string, label: string, version: number, o?: ClientOptions) =>
    request<Prompt>(`/v1/api/prompts/${enc(name)}/labels/${enc(label)}${q({ project })}`, o, { version }, 'PUT'),
  unlabel: (project: string, name: string, label: string, o?: ClientOptions) =>
    request<DeleteResult>(`/v1/api/prompts/${enc(name)}/labels/${enc(label)}${q({ project })}`, o, undefined, 'DELETE'),
  spans: (project: string, name: string, version: number, o?: ClientOptions) =>
    request<SpanListResponse>(`/v1/api/spans${q({ project, prompt: name, prompt_version: version, limit: 20 })}`, o),
}

/** The body of a version as lines, so two versions can be compared line by line. */
function bodyText(p: Prompt): string {
  if (p.kind === 'text') return p.template ?? ''
  const msgs = Array.isArray(p.messages) ? (p.messages as { role?: string; content?: unknown }[]) : []
  return msgs
    .map((m) => `[${m.role ?? '?'}]\n${typeof m.content === 'string' ? m.content : JSON.stringify(m.content, null, 2)}`)
    .join('\n\n')
}

type DiffLine = { op: ' ' | '+' | '-'; text: string }

/** A line diff by longest common subsequence. Prompts are short, so the square table is cheap. */
export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split('\n')
  const b = after.split('\n')
  const n = a.length
  const m = b.length
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1])
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ op: ' ', text: a[i] })
      i++
      j++
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ op: '-', text: a[i++] })
    } else {
      out.push({ op: '+', text: b[j++] })
    }
  }
  while (i < n) out.push({ op: '-', text: a[i++] })
  while (j < m) out.push({ op: '+', text: b[j++] })
  return out
}

export default function Prompts({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [list, setList] = useState<Prompt[] | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [versions, setVersions] = useState<Prompt[] | null>(null)
  const [selected, setSelected] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)
  const refresh = useCallback(() => setTick((t) => t + 1), [])

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    setError(null)
    prompts
      .list(project, { endpoint, signal: ac.signal })
      .then(setList)
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [conn.state, endpoint, project, tick])

  useEffect(() => {
    if (!open) {
      setVersions(null)
      return
    }
    const ac = new AbortController()
    prompts
      .versions(project, open, { endpoint, signal: ac.signal })
      .then((r) => {
        setVersions(r.versions)
        setSelected((s) => (s !== null && r.versions.some((v) => v.version === s) ? s : (r.versions[0]?.version ?? null)))
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [open, project, endpoint, tick])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  const current = versions?.find((v) => v.version === selected) ?? null
  const previous = current && versions ? versions.find((v) => v.version < current.version) : undefined

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={list ? `${list.length} prompts` : ''}
      />
      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && <Failure title="Request refused" error={error} />}

        <Section title="Prompts">
          {list && list.length === 0 ? (
            <Empty>
              No prompt in this project. Store one with <code>POST /v1/api/prompts</code> or an SDK&rsquo;s prompt
              helper, then fetch it by name and label from your code.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Prompt</th>
                    <th className="px-3 py-2">Latest</th>
                    <th className="px-3 py-2">Labels on it</th>
                    <th className="px-3 py-2">Variables</th>
                    <th className="px-3 py-2">Stored</th>
                    <th className="px-3 py-2">Note</th>
                  </tr>
                </thead>
                <tbody>
                  {list?.map((p) => (
                    <tr key={p.name} className="border-t border-scribe">
                      <td className="px-3 py-1.5">
                        <button
                          type="button"
                          className="link data"
                          aria-expanded={open === p.name}
                          onClick={() => setOpen(open === p.name ? null : p.name)}
                        >
                          {p.name}
                        </button>
                      </td>
                      <td className="data px-3 py-1.5">v{p.version}</td>
                      <td className="data px-3 py-1.5 text-ink-2">{p.labels.join(', ')}</td>
                      <td className="data px-3 py-1.5 text-ink-2">{p.variables.join(', ')}</td>
                      <td className="data px-3 py-1.5 whitespace-nowrap">{when(p.created_at)}</td>
                      <td className="max-w-[28rem] px-3 py-1.5 text-ink-2">{p.note ?? ''}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>

        <Section title="New version">
          <Compose
            key={`${open ?? ''}-${current?.version ?? 0}`}
            endpoint={endpoint}
            project={project}
            base={current}
            onSaved={(p) => {
              setOpen(p.name)
              setSelected(p.version)
              refresh()
            }}
          />
        </Section>

        {open && versions && (
          <Section title={`Versions of ${open}`}>
            <div className="grid gap-4 lg:grid-cols-[18rem_minmax(0,1fr)]">
              <ol className="panel divide-y divide-scribe self-start">
                {versions.map((v) => (
                  <li key={v.version}>
                    <button
                      type="button"
                      aria-current={v.version === selected}
                      onClick={() => setSelected(v.version)}
                      className={`block w-full px-3 py-2 text-left text-[0.8125rem] transition-colors hover:bg-panel-2 ${
                        v.version === selected ? 'bg-panel-2 text-ink' : 'text-ink-2'
                      }`}
                    >
                      <span className="data font-medium">v{v.version}</span>
                      {v.labels.length > 0 && <span className="data ml-2 text-ink-3">{v.labels.join(', ')}</span>}
                      <span className="data block text-[0.75rem] text-ink-3">{when(v.created_at)}</span>
                      {v.note && <span className="block truncate text-[0.75rem] text-ink-3">{v.note}</span>}
                    </button>
                  </li>
                ))}
              </ol>
              {current && (
                <VersionDetail
                  endpoint={endpoint}
                  project={project}
                  prompt={current}
                  previous={previous}
                  onChanged={refresh}
                  onError={setError}
                  onOpenTrace={onOpenTrace}
                />
              )}
            </div>
          </Section>
        )}
      </div>
    </div>
  )
}

function VersionDetail({
  endpoint,
  project,
  prompt,
  previous,
  onChanged,
  onError,
  onOpenTrace,
}: {
  endpoint: string
  project: string
  prompt: Prompt
  previous: Prompt | undefined
  onChanged: () => void
  onError: (e: string) => void
  onOpenTrace: (id: string) => void
}) {
  const [label, setLabel] = useState('production')
  const [busy, setBusy] = useState(false)
  const [spans, setSpans] = useState<SpanListResponse | null>(null)
  const diff = useMemo(
    () => (previous ? diffLines(bodyText(previous), bodyText(prompt)) : null),
    [previous, prompt],
  )

  useEffect(() => {
    const ac = new AbortController()
    setSpans(null)
    prompts
      .spans(project, prompt.name, prompt.version, { endpoint, signal: ac.signal })
      .then(setSpans)
      .catch(() => setSpans({ spans: [], page: { has_more: false } }))
    return () => ac.abort()
  }, [endpoint, project, prompt.name, prompt.version])

  const act = (p: Promise<unknown>) => {
    setBusy(true)
    p.then(onChanged)
      .catch((e) => onError(message(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="min-w-0 space-y-4">
      <div className="panel p-4">
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[0.8125rem]">
          <span className="data font-medium text-ink">
            {prompt.name} v{prompt.version}
          </span>
          <span className="text-ink-3">{prompt.kind === 'chat' ? 'chat messages' : 'text template'}</span>
          {prompt.created_by && <span className="data text-ink-3">stored by {prompt.created_by}</span>}
        </div>
        {prompt.note && <p className="mt-2 text-[0.8125rem] text-ink-2">{prompt.note}</p>}
        <Body prompt={prompt} />
        {prompt.config != null && (
          <>
            <p className="legend mt-4">Model settings</p>
            <pre className="data well mt-2 overflow-x-auto p-3 text-[0.75rem] text-ink-2">
              {JSON.stringify(prompt.config, null, 2)}
            </pre>
          </>
        )}
      </div>

      <div className="panel p-4">
        <p className="legend">Labels</p>
        <p className="mt-2 text-[0.8125rem] text-ink-2">
          {prompt.labels.length === 0 ? 'No label points at this version.' : 'Pointing here: '}
          {prompt.labels.map((l, i) => (
            <span key={l}>
              {i > 0 && ', '}
              <span className="data text-ink">{l}</span>{' '}
              <button
                type="button"
                className="link text-ink-3"
                disabled={busy}
                onClick={() => act(prompts.unlabel(project, prompt.name, l, { endpoint }))}
              >
                remove
              </button>
            </span>
          ))}
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1.5 text-[0.8125rem]">
            <span className="sr-only">Label</span>
            <input
              className="field w-40"
              value={label}
              onChange={(e) => setLabel(e.target.value.trim())}
              placeholder="production"
            />
          </label>
          <button
            type="button"
            disabled={busy || !label}
            onClick={() => act(prompts.label(project, prompt.name, label, prompt.version, { endpoint }))}
            className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
          >
            Point at v{prompt.version}
          </button>
        </div>
      </div>

      {diff && previous && (
        <div className="panel p-4">
          <p className="legend">Changes from v{previous.version}</p>
          <pre className="data well mt-2 overflow-x-auto p-3 text-[0.75rem] leading-relaxed">
            {diff.map((d, i) => (
              <div
                key={i}
                className={d.op === '+' ? 'text-olive' : d.op === '-' ? 'text-oxide line-through' : 'text-ink-3'}
              >
                <span aria-hidden="true">{d.op} </span>
                <span className="sr-only">{d.op === '+' ? 'added: ' : d.op === '-' ? 'removed: ' : ''}</span>
                {d.text || ' '}
              </div>
            ))}
          </pre>
        </div>
      )}

      <div className="panel p-4">
        <p className="legend">Steps that ran this version</p>
        {spans === null ? (
          <p className="mt-2 text-[0.8125rem] text-ink-3">Reading…</p>
        ) : spans.spans.length === 0 ? (
          <p className="mt-2 text-[0.8125rem] text-ink-3">
            No step is linked to this version yet. Link one with your SDK&rsquo;s prompt helper on the model span.
          </p>
        ) : (
          <ul className="mt-2 space-y-1 text-[0.8125rem]">
            {spans.spans.map((s) => (
              <li key={s.span_id} className="flex flex-wrap gap-x-3">
                <button type="button" className="link data" onClick={() => onOpenTrace(s.trace_id)}>
                  {s.trace_id.slice(0, 12)}
                </button>
                <span className="text-ink-2">{s.name}</span>
                <span className="data text-ink-3">{when(s.start_time)}</span>
                {s.status === 'error' && <span className="text-oxide">failed</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}

function Body({ prompt }: { prompt: Prompt }) {
  if (prompt.kind === 'text') {
    return (
      <pre className="data well mt-3 overflow-x-auto p-3 text-[0.8125rem] whitespace-pre-wrap text-ink">
        {prompt.template}
      </pre>
    )
  }
  const msgs = Array.isArray(prompt.messages) ? (prompt.messages as { role?: string; content?: unknown }[]) : []
  return (
    <ol className="mt-3 space-y-2">
      {msgs.map((m, i) => (
        <li key={i} className="well p-3">
          <p className="legend">{m.role ?? 'message'}</p>
          <pre className="data mt-1 text-[0.8125rem] whitespace-pre-wrap text-ink">
            {typeof m.content === 'string' ? m.content : JSON.stringify(m.content, null, 2)}
          </pre>
        </li>
      ))}
    </ol>
  )
}

/** Store a new version: a new prompt, or the next version of the one open, prefilled from it. */
function Compose({
  endpoint,
  project,
  base,
  onSaved,
}: {
  endpoint: string
  project: string
  base: Prompt | null
  onSaved: (p: Prompt) => void
}) {
  const [name, setName] = useState(base?.name ?? '')
  const [kind, setKind] = useState<'text' | 'chat'>(base?.kind ?? 'text')
  const [text, setText] = useState(
    base ? (base.kind === 'text' ? (base.template ?? '') : JSON.stringify(base.messages, null, 2)) : '',
  )
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState<string | null>(null)

  const save = () => {
    setError(null)
    setSaved(null)
    let body: Record<string, unknown> = { name, kind, note: note || undefined, config: base?.config ?? undefined }
    if (kind === 'text') body = { ...body, template: text }
    else {
      try {
        body = { ...body, messages: JSON.parse(text) }
      } catch {
        setError('The messages are not valid JSON. Write an array such as [{"role": "system", "content": "…"}].')
        return
      }
    }
    setBusy(true)
    prompts
      .create(project, body, { endpoint })
      .then((p) => {
        setSaved(
          base && p.version === base.version && p.name === base.name
            ? `Nothing changed, so v${p.version} stays the latest.`
            : `Stored as ${p.name} v${p.version}.`,
        )
        onSaved(p)
      })
      .catch((e) => setError(message(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="panel p-4">
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Name
          <input className="field data w-64" value={name} onChange={(e) => setName(e.target.value)} placeholder="support/triage" />
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Shape
          <select className="field" value={kind} onChange={(e) => setKind(e.target.value as 'text' | 'chat')}>
            <option value="text">text template</option>
            <option value="chat">chat messages (JSON)</option>
          </select>
        </label>
      </div>
      <label className="mt-3 block text-[0.8125rem]">
        <span className="sr-only">Body</span>
        <textarea
          className="field data block min-h-40 w-full"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={kind === 'text' ? 'Answer the question about {{topic}}.' : '[{"role": "system", "content": "…"}]'}
          spellCheck={false}
        />
      </label>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <label className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem]">
          Note
          <input className="field w-full" value={note} onChange={(e) => setNote(e.target.value)} placeholder="What changed" />
        </label>
        <button
          type="button"
          disabled={busy || !name || !text.trim()}
          onClick={save}
          className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
        >
          {busy ? 'Storing…' : 'Store version'}
        </button>
      </div>
      <p className="mt-2 text-[0.75rem] text-ink-3">
        Write variables as <code>{'{{name}}'}</code>. A new version gets no label until you point one at it.
      </p>
      {saved && <p className="mt-2 text-[0.8125rem] text-ink-2">{saved}</p>}
      {error && <Failure title="Not stored" error={error} />}
    </div>
  )
}
