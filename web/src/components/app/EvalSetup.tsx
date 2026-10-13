/**
 * The two things a project sets up for evaluation: its own scorers, and the
 * rules that score runs live as they finish.
 *
 * Both are stored as versions. Saving an edit stores the next version; a
 * score always names the version that produced it, so changing a judge
 * prompt never silently changes what old scores meant.
 */

import { useState } from 'react'
import { request } from '@/lib/api'
import type { ClientOptions } from '@/lib/api'
import type {
  CreateEvaluationRuleRequest,
  CreateScorerDefinitionRequest,
  EvaluationRule,
  ScorerDefinition,
  ScorerInfo,
} from '@/generated/types'
import { Empty, Failure, message, when } from './screen-kit'

const enc = encodeURIComponent

export const evalSetup = {
  definitions: (project: string, o?: ClientOptions) =>
    request<ScorerDefinition[]>(`/v1/api/scorer-definitions?project=${enc(project)}`, o),
  saveDefinition: (project: string, body: CreateScorerDefinitionRequest, o?: ClientOptions) =>
    request<ScorerDefinition>(`/v1/api/scorer-definitions?project=${enc(project)}`, o, body),
  rules: (project: string, o?: ClientOptions) =>
    request<EvaluationRule[]>(`/v1/api/evaluation-rules?project=${enc(project)}`, o),
  saveRule: (project: string, body: CreateEvaluationRuleRequest, o?: ClientOptions) =>
    request<EvaluationRule>(`/v1/api/evaluation-rules?project=${enc(project)}`, o, body),
}

/** A rule's body as a request, so a toggle posts the same rule with one change. */
function ruleRequest(r: EvaluationRule): CreateEvaluationRuleRequest {
  return {
    id: r.id,
    enabled: r.enabled,
    scorers: r.scorers,
    sample_rate: r.sample_rate,
    name: r.name ?? undefined,
    kind: r.kind ?? undefined,
    environment: r.environment ?? undefined,
    tags: r.tags,
    status: r.status ?? undefined,
    config: r.config ?? undefined,
    max_judged_per_minute: r.max_judged_per_minute,
  }
}

function filterText(r: EvaluationRule): string {
  const parts: string[] = []
  if (r.name) parts.push(`name ${r.name}`)
  if (r.kind) parts.push(`kind ${r.kind}`)
  if (r.environment) parts.push(`environment ${r.environment}`)
  if (r.status) parts.push(`outcome ${r.status}`)
  if (r.tags.length) parts.push(`tags ${r.tags.join(', ')}`)
  return parts.length ? parts.join('; ') : 'every finished run'
}

export function LiveRules({
  endpoint,
  project,
  rules,
  scorers,
  onChanged,
}: {
  endpoint: string
  project: string
  rules: EvaluationRule[] | null
  scorers: ScorerInfo[]
  onChanged: () => void
}) {
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const toggle = (r: EvaluationRule) => {
    setBusy(r.id)
    setError(null)
    evalSetup
      .saveRule(project, { ...ruleRequest(r), enabled: !r.enabled, note: r.enabled ? 'turned off' : 'turned on' }, { endpoint })
      .then(onChanged)
      .catch((e) => setError(message(e)))
      .finally(() => setBusy(null))
  }

  return (
    <div className="space-y-4">
      {rules && rules.length === 0 ? (
        <Empty>
          No live scoring rule in this project. A rule scores runs as they finish, with the scorers you name, on
          the share of runs you choose.
        </Empty>
      ) : (
        <div className="panel overflow-x-auto">
          <table className="w-full text-left text-[0.8125rem]">
            <thead className="legend text-ink-3">
              <tr>
                <th className="px-3 py-2">Rule</th>
                <th className="px-3 py-2">State</th>
                <th className="px-3 py-2">Scorers</th>
                <th className="px-3 py-2">Share</th>
                <th className="px-3 py-2">Runs</th>
                <th className="px-3 py-2">Judged a minute</th>
                <th className="px-3 py-2">Version</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {rules?.map((r) => (
                <tr key={r.id} className="border-t border-scribe align-top">
                  <td className="data px-3 py-1.5">{r.id}</td>
                  <td className={`px-3 py-1.5 ${r.enabled ? 'text-olive' : 'text-ink-3'}`}>{r.enabled ? 'on' : 'off'}</td>
                  <td className="data px-3 py-1.5 text-ink-2">{r.scorers.join(', ')}</td>
                  <td className="data px-3 py-1.5">{Math.round(r.sample_rate * 1000) / 10}%</td>
                  <td className="px-3 py-1.5 text-ink-2">{filterText(r)}</td>
                  <td className="data px-3 py-1.5">{r.max_judged_per_minute}</td>
                  <td className="data px-3 py-1.5 whitespace-nowrap" title={when(r.created_at)}>
                    v{r.version}
                  </td>
                  <td className="px-3 py-1.5">
                    <button type="button" className="link" disabled={busy === r.id} onClick={() => toggle(r)}>
                      {r.enabled ? 'turn off' : 'turn on'}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {error && <Failure title="Not stored" error={error} />}
      <NewRule endpoint={endpoint} project={project} scorers={scorers} onSaved={onChanged} />
    </div>
  )
}

function NewRule({
  endpoint,
  project,
  scorers,
  onSaved,
}: {
  endpoint: string
  project: string
  scorers: ScorerInfo[]
  onSaved: () => void
}) {
  const runnable = scorers.filter((s) => s.available && !s.needs_config)
  const [id, setId] = useState('')
  const [chosen, setChosen] = useState<string[]>([])
  const [share, setShare] = useState(10)
  const [name, setName] = useState('')
  const [environment, setEnvironment] = useState('')
  const [enabled, setEnabled] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const toggle = (s: string) => setChosen((c) => (c.includes(s) ? c.filter((x) => x !== s) : [...c, s]))
  const judged = chosen.filter((c) => scorers.find((s) => s.id === c)?.kind !== 'code').length

  const save = () => {
    setBusy(true)
    setError(null)
    evalSetup
      .saveRule(
        project,
        {
          id,
          enabled,
          scorers: chosen,
          sample_rate: share / 100,
          name: name || undefined,
          environment: environment || undefined,
        },
        { endpoint },
      )
      .then(() => {
        setId('')
        setChosen([])
        onSaved()
      })
      .catch((e) => setError(message(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="panel p-4">
      <p className="legend">New rule</p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Id
          <input className="field data w-44" value={id} onChange={(e) => setId(e.target.value)} placeholder="checkout-quality" />
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Score
          <input
            type="number"
            min={0}
            max={100}
            className="field w-20"
            value={share}
            onChange={(e) => setShare(Math.max(0, Math.min(100, Number(e.target.value) || 0)))}
          />
          % of runs
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Run name
          <input className="field w-36" value={name} onChange={(e) => setName(e.target.value)} placeholder="any" />
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Environment
          <input className="field w-32" value={environment} onChange={(e) => setEnvironment(e.target.value)} placeholder="any" />
        </label>
      </div>
      <div className="mt-3 flex flex-wrap gap-x-4 gap-y-2">
        {runnable.map((s) => (
          <label key={s.id} className="flex items-center gap-1.5 text-[0.8125rem]" title={s.description}>
            <input type="checkbox" checked={chosen.includes(s.id)} onChange={() => toggle(s.id)} />
            <span className="data">{s.id}</span>
            {s.kind !== 'code' && <span className="text-ink-3">judged</span>}
          </label>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
          turn it on now
        </label>
        <button
          type="button"
          disabled={busy || !id || chosen.length === 0}
          onClick={save}
          className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
        >
          {busy ? 'Storing…' : 'Store rule'}
        </button>
        {judged > 0 && (
          <span className="text-[0.75rem] text-ink-3">
            {judged} judged scorer{judged > 1 ? 's' : ''}: one model call each per scored run, at most 30 a minute.
          </span>
        )}
      </div>
      {error && <Failure title="Not stored" error={error} />}
    </div>
  )
}

export function ScorerDefinitions({
  endpoint,
  project,
  definitions,
  scorers,
  onChanged,
}: {
  endpoint: string
  project: string
  definitions: ScorerDefinition[] | null
  scorers: ScorerInfo[]
  onChanged: () => void
}) {
  const [editing, setEditing] = useState<ScorerDefinition | null>(null)
  const [formKey, setFormKey] = useState(0)

  return (
    <div className="space-y-4">
      {definitions && definitions.length === 0 ? (
        <Empty>
          No scorer defined in this project. Define a judge prompt written for your agent, or fix the settings of a
          code scorer, and name it in a rule or a scoring pass like any other scorer.
        </Empty>
      ) : (
        <div className="panel overflow-x-auto">
          <table className="w-full text-left text-[0.8125rem]">
            <thead className="legend text-ink-3">
              <tr>
                <th className="px-3 py-2">Scorer</th>
                <th className="px-3 py-2">Kind</th>
                <th className="px-3 py-2">Version</th>
                <th className="px-3 py-2">Better</th>
                <th className="px-3 py-2">Measures</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {definitions?.map((d) => (
                <tr key={d.id} className="border-t border-scribe align-top">
                  <td className="data px-3 py-1.5">{d.id}</td>
                  <td className="px-3 py-1.5">{d.kind === 'judge' ? 'judge prompt' : `code, ${d.builtin ?? ''}`}</td>
                  <td className="data px-3 py-1.5" title={when(d.created_at)}>
                    v{d.version}
                  </td>
                  <td className="px-3 py-1.5">{d.direction === 'lower_is_better' ? 'lower' : 'higher'}</td>
                  <td className="max-w-[36rem] px-3 py-1.5 text-ink-2">{d.description}</td>
                  <td className="px-3 py-1.5">
                    <button
                      type="button"
                      className="link"
                      onClick={() => {
                        setEditing(d)
                        setFormKey((k) => k + 1)
                      }}
                    >
                      edit
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <DefinitionForm
        key={formKey}
        endpoint={endpoint}
        project={project}
        base={editing}
        scorers={scorers}
        onSaved={() => {
          setEditing(null)
          setFormKey((k) => k + 1)
          onChanged()
        }}
      />
    </div>
  )
}

function DefinitionForm({
  endpoint,
  project,
  base,
  scorers,
  onSaved,
}: {
  endpoint: string
  project: string
  base: ScorerDefinition | null
  scorers: ScorerInfo[]
  onSaved: () => void
}) {
  const codes = scorers.filter((s) => s.kind === 'code' && s.needs_config)
  const [id, setId] = useState(base?.id ?? '')
  const [kind, setKind] = useState<'judge' | 'code'>(base?.kind ?? 'judge')
  const [description, setDescription] = useState(base?.description ?? '')
  const [lower, setLower] = useState(base?.direction === 'lower_is_better')
  const [prompt, setPrompt] = useState(base?.prompt ?? '')
  const [builtin, setBuiltin] = useState(base?.builtin ?? codes[0]?.id ?? '')
  const [config, setConfig] = useState(base?.config ? JSON.stringify(base.config, null, 2) : '{}')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const keys = codes.find((c) => c.id === builtin)?.config_keys ?? []

  const save = () => {
    setError(null)
    const body: CreateScorerDefinitionRequest = {
      id,
      kind,
      description,
      direction: lower ? 'lower_is_better' : 'higher_is_better',
      note: note || undefined,
    }
    if (kind === 'judge') body.prompt = prompt
    else {
      body.builtin = builtin
      try {
        body.config = JSON.parse(config)
      } catch {
        setError('The settings are not valid JSON. Write an object such as {"keywords": ["refund"]}.')
        return
      }
    }
    setBusy(true)
    evalSetup
      .saveDefinition(project, body, { endpoint })
      .then(onSaved)
      .catch((e) => setError(message(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="panel p-4">
      <p className="legend">{base ? `Next version of ${base.id}` : 'New scorer'}</p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Id
          <input
            className="field data w-44"
            value={id}
            disabled={!!base}
            onChange={(e) => setId(e.target.value)}
            placeholder="cites_policy"
          />
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          Kind
          <select className="field" value={kind} disabled={!!base} onChange={(e) => setKind(e.target.value as 'judge' | 'code')}>
            <option value="judge">judge prompt</option>
            <option value="code">code scorer with settings</option>
          </select>
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          <input type="checkbox" checked={lower} onChange={(e) => setLower(e.target.checked)} />
          lower is better
        </label>
      </div>
      <label className="mt-3 flex items-center gap-1.5 text-[0.8125rem]">
        Measures
        <input
          className="field w-full"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="The answer cites the refund policy by section."
        />
      </label>
      {kind === 'judge' ? (
        <label className="mt-3 block text-[0.8125rem]">
          <span className="text-ink-3">The criterion the judge grades against, from 0 to 1</span>
          <textarea
            className="field mt-1 block min-h-28 w-full"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="The answer names the section of the refund policy it relies on."
          />
        </label>
      ) : (
        <div className="mt-3 grid gap-3 sm:grid-cols-[16rem_minmax(0,1fr)]">
          <label className="flex items-center gap-1.5 self-start text-[0.8125rem]">
            Scorer
            <select className="field" value={builtin} onChange={(e) => setBuiltin(e.target.value)}>
              {codes.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.id}
                </option>
              ))}
            </select>
          </label>
          <label className="block text-[0.8125rem]">
            <span className="text-ink-3">Settings, as JSON. This scorer reads {keys.join(', ')}.</span>
            <textarea
              className="field data mt-1 block min-h-24 w-full"
              value={config}
              onChange={(e) => setConfig(e.target.value)}
              spellCheck={false}
            />
          </label>
        </div>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem]">
          Note
          <input className="field w-full" value={note} onChange={(e) => setNote(e.target.value)} placeholder="What changed" />
        </label>
        <button
          type="button"
          disabled={busy || !id || !description}
          onClick={save}
          className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
        >
          {busy ? 'Storing…' : base ? 'Store next version' : 'Store scorer'}
        </button>
      </div>
      {error && <Failure title="Not stored" error={error} />}
    </div>
  )
}
