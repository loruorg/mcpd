/**
 * The monitors screen.
 *
 * Every monitor and objective this daemon evaluates, the condition each is
 * latched in, and the reading that put it there. A tripped condition stays
 * tripped until it clears or somebody acknowledges it, so the acknowledge
 * control is here, beside the reading it answers.
 */

import { useEffect, useState } from 'react'
import type { MonitorCondition, MonitorState, SloState } from '@/generated/types'
import { Disconnected } from './Explorer'
import { useConnection } from './shell'
import { Empty, Failure, fixed, message, more, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

const TONE: Record<MonitorCondition, string> = {
  nominal: 'text-ink-2',
  tripped: 'text-oxide',
  acknowledged: 'text-ink',
  insufficient: 'text-ink-3',
}

export default function Monitors({ endpoint, onEndpoint, project, onProject }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [monitors, setMonitors] = useState<MonitorState[] | null>(null)
  const [slos, setSlos] = useState<SloState[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setError(null)
    Promise.all([more.monitors(o), more.slos(o)])
      .then(([m, s]) => {
        setMonitors(m)
        setSlos(s)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    // Conditions move on the daemon's timer, so the screen follows it.
    const t = setInterval(() => setNonce((n) => n + 1), 30_000)
    return () => {
      ac.abort()
      clearInterval(t)
    }
  }, [conn.state, endpoint, nonce])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  const ack = (kind: 'monitor' | 'slo', id: string) => {
    const call = kind === 'monitor' ? more.ackMonitor(id, { endpoint }) : more.ackSlo(id, { endpoint })
    call.then(() => setNonce((n) => n + 1)).catch((e) => setError(message(e)))
  }

  const mine = (p: string) => !project || p === project
  const shownMonitors = monitors?.filter((m) => mine(m.monitor.project))
  const shownSlos = slos?.filter((s) => mine(s.slo.project))

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={monitors ? `${shownMonitors?.length ?? 0} monitors, ${shownSlos?.length ?? 0} objectives` : ''}
      />
      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && <Failure title="Query failed" error={error} />}

        <Section title="Monitors">
          {shownMonitors && shownMonitors.length === 0 ? (
            <Empty>
              No monitor watches this project. Monitors are set under <code>monitors</code> in the config file.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Monitor</th>
                    <th className="px-3 py-2">Watches</th>
                    <th className="px-3 py-2">Condition</th>
                    <th className="px-3 py-2">Last reading</th>
                    <th className="px-3 py-2" />
                  </tr>
                </thead>
                <tbody>
                  {shownMonitors?.map((m) => (
                    <tr key={m.monitor.id} className="border-t border-scribe align-top">
                      <td className="px-3 py-1.5">
                        {m.monitor.name}
                        {!m.monitor.enabled && <span className="text-ink-3">, disabled</span>}
                      </td>
                      <td className="data px-3 py-1.5">
                        {m.monitor.watch}
                        {m.monitor.scorer_id ? `(${m.monitor.scorer_id})` : ''} {m.monitor.compare}{' '}
                        {m.monitor.threshold} over {m.monitor.window_seconds}s
                      </td>
                      <td className={`px-3 py-1.5 ${TONE[m.condition]}`}>
                        {m.condition}
                        {m.tripped_at && <span className="data text-ink-3"> since {when(m.tripped_at)}</span>}
                      </td>
                      <td className="px-3 py-1.5 text-ink-2">
                        {m.last ? (
                          <>
                            <span className="data">{fixed(m.last.value)}</span> from {m.last.samples} runs:{' '}
                            {m.last.detail}
                          </>
                        ) : (
                          'not evaluated yet'
                        )}
                      </td>
                      <td className="px-3 py-1.5">
                        {m.condition === 'tripped' && (
                          <button type="button" className="link" onClick={() => ack('monitor', m.monitor.id)}>
                            acknowledge
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>

        <Section title="Service level objectives">
          {shownSlos && shownSlos.length === 0 ? (
            <Empty>
              No objective covers this project. Objectives are set under <code>slos</code> in the config file.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Objective</th>
                    <th className="px-3 py-2">Target</th>
                    <th className="px-3 py-2">Condition</th>
                    <th className="px-3 py-2">Success</th>
                    <th className="px-3 py-2">Budget left</th>
                    <th className="px-3 py-2">Burn 1h / 6h</th>
                    <th className="px-3 py-2" />
                  </tr>
                </thead>
                <tbody>
                  {shownSlos?.map((s) => (
                    <tr key={s.slo.id} className="border-t border-scribe align-top">
                      <td className="px-3 py-1.5">{s.slo.name}</td>
                      <td className="data px-3 py-1.5">
                        {fixed(s.slo.objective * 100, 2)}% over {s.slo.window_days} days
                      </td>
                      <td className={`px-3 py-1.5 ${TONE[s.condition]}`}>{s.condition}</td>
                      <td className="data px-3 py-1.5">
                        {s.last?.success_rate == null ? '' : `${fixed(s.last.success_rate * 100, 2)}%`}
                      </td>
                      <td className="data px-3 py-1.5">
                        {s.last?.budget_remaining == null ? '' : `${fixed(s.last.budget_remaining * 100, 1)}%`}
                      </td>
                      <td className="data px-3 py-1.5">
                        {fixed(s.last?.burn_fast, 2)} / {fixed(s.last?.burn_slow, 2)}
                      </td>
                      <td className="px-3 py-1.5">
                        {s.condition === 'tripped' && (
                          <button type="button" className="link" onClick={() => ack('slo', s.slo.id)}>
                            acknowledge
                          </button>
                        )}
                      </td>
                    </tr>
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
