/**
 * The account panel, for a daemon started with --signup or --accounts: keys,
 * security and organisations. The screens before a session are in
 * `AccountAuth`.
 *
 * Shown only when the daemon's health probe says `signup` or `accounts`. A
 * daemon without either never sees any of this, and the interface reads it
 * exactly as it always has.
 *
 * The session token is kept in this browser's storage with the endpoint that
 * issued it, and `lib/api` sends it with every request to that endpoint. A key
 * secret is shown once, when it is made, and never stored here.
 */

import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { accountCall, api, clearSession } from '@/lib/api'
import type { StoredSession } from '@/lib/api'
import { ALL_KEY_SCOPE } from '@/generated/types'
import type { Account, AccountKey, CreatedAccountKey, KeyScope, Org, OrgProject } from '@/generated/types'
import { failureText, InviteLink, linkFromUrl, PENDING_INVITE } from './AccountAuth'
import { SecurityPanel } from './AccountSecurity'
import { OrgsPanel } from './Orgs'

type Tab = 'keys' | 'security' | 'orgs'

const SCOPE_TEXT: Record<KeyScope, string> = {
  ingest: 'write signals from your services',
  read: 'read runs, logs and spend',
  write: 'record scores, feedback and datasets',
  admin: 'delete data and read the audit log',
  execute: 'start, cancel and signal runs',
  worker: 'claim and finish steps',
}

export { AuthScreen } from './AccountAuth'

/* ------------------------------------------------------------- account --- */

export function AccountPanel({
  endpoint,
  session,
  onLogout,
}: {
  endpoint: string
  session: StoredSession
  onLogout: () => void
}) {
  const [account, setAccount] = useState<Account | null>(null)
  const [keys, setKeys] = useState<AccountKey[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [fresh, setFresh] = useState<CreatedAccountKey | null>(null)
  const [scope, setScope] = useState<KeyScope>('ingest')
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('keys')
  const [projects, setProjects] = useState<{ project: string; label: string }[]>([])
  const [keyProject, setKeyProject] = useState('')
  // An invitation opened while logged in, or opened while logged out and kept
  // across the log-in.
  const [invite, setInvite] = useState<string | null>(() => {
    const l = linkFromUrl()
    if (l?.kind === 'invite' && l.token) return l.token
    try {
      return sessionStorage.getItem(PENDING_INVITE)
    } catch {
      return null
    }
  })

  const load = useCallback(async () => {
    try {
      const [a, k] = await Promise.all([api.account({ endpoint }), api.accountKeys({ endpoint })])
      setAccount(a)
      setKeys(k)
      setError(null)
      // Every project the account can make a key on, by organisation.
      const orgs = await accountCall<Org[]>('GET', '/v1/api/orgs', { endpoint })
      const lists = await Promise.all(
        orgs.map((org) =>
          accountCall<OrgProject[]>('GET', `/v1/api/orgs/${encodeURIComponent(org.id)}/projects`, { endpoint })
            .then((ps) => ps.map((p) => ({ project: p.project, label: `${org.name}: ${p.label ?? p.project}` })))
            .catch(() => []),
        ),
      )
      setProjects(lists.flat())
    } catch (e) {
      setError(failureText(e))
    }
  }, [endpoint])

  useEffect(() => {
    void load()
  }, [load])

  const create = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    try {
      const made = await api.createAccountKey(scope, name.trim() || undefined, { endpoint }, keyProject || undefined)
      setFresh(made)
      setName('')
      await load()
    } catch (err) {
      setError(failureText(err))
    } finally {
      setBusy(false)
    }
  }

  const revoke = async (id: string) => {
    setConfirming(null)
    try {
      await api.revokeAccountKey(id, { endpoint })
      if (fresh?.key.id === id) setFresh(null)
      await load()
    } catch (err) {
      setError(failureText(err))
    }
  }

  const logout = async () => {
    // The session ends here whatever the daemon says: a reader who asked to
    // log out must not be left holding a token because the network failed.
    await api.logout({ endpoint }).catch(() => undefined)
    clearSession()
    onLogout()
  }

  const project = (keyProject || account?.project || session.project) ?? '<your project>'
  const ingestSecret = fresh?.key.scope === 'ingest' ? fresh.secret : '<an ingest key>'
  const snippet = [
    `MCPD_ENDPOINT=${endpoint}`,
    `MCPD_API_KEY=${ingestSecret}`,
    `MCPD_PROJECT=${project}`,
  ].join('\n')
  const otlp = [
    `OTEL_EXPORTER_OTLP_ENDPOINT=${endpoint}`,
    `OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer ${ingestSecret}"`,
  ].join('\n')

  return (
    <div className="mx-auto max-w-[60rem] px-4 py-8 sm:px-6">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 className="display text-[1.5rem]">Account</h2>
        <button
          type="button"
          onClick={logout}
          className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
        >
          Log out
        </button>
      </div>

      {error && (
        <p role="alert" className="mt-4 text-[0.875rem] leading-relaxed text-oxide">
          {error}
        </p>
      )}

      {invite && (
        <section className="mt-6">
          <h3 className="legend text-ink-3">An invitation</h3>
          <InviteLink
            endpoint={endpoint}
            token={invite}
            loggedIn
            onAccepted={() => {
              try {
                sessionStorage.removeItem(PENDING_INVITE)
              } catch {
                /* Nothing was kept. */
              }
              setInvite(null)
              setTab('orgs')
              void load()
            }}
          />
        </section>
      )}

      <dl className="panel mt-6 grid gap-x-8 gap-y-4 p-5 sm:grid-cols-2">
        <div className="min-w-0">
          <dt className="legend text-ink-3">Email</dt>
          <dd className="mt-1 truncate text-[0.9375rem] text-ink">{account?.email ?? session.email}</dd>
        </div>
        <div className="min-w-0">
          <dt className="legend text-ink-3">Project</dt>
          <dd className="data mt-1 truncate text-[0.875rem] text-ink">{project}</dd>
        </div>
      </dl>

      <nav aria-label="Account sections" className="mt-8 flex gap-6 border-b border-scribe pb-2">
        {(
          [
            ['keys', 'Keys'],
            ['security', 'Security'],
            ['orgs', 'Organisations'],
          ] as [Tab, string][]
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            onClick={() => setTab(id)}
            aria-current={tab === id ? 'page' : undefined}
            className={`display text-[0.9375rem] transition-colors ${tab === id ? 'text-ink' : 'font-medium text-ink-3 hover:text-ink'}`}
          >
            {label}
          </button>
        ))}
      </nav>

      {tab === 'security' && account && (
        <SecurityPanel endpoint={endpoint} account={account} onChanged={() => void load()} onDeleted={onLogout} />
      )}
      {tab === 'orgs' && <OrgsPanel endpoint={endpoint} />}

      {tab === 'keys' && (<>
      <section className="mt-9">
        <h3 className="legend text-ink-3">API keys</h3>
        <p className="mt-2 max-w-[62ch] text-[0.875rem] leading-relaxed text-ink-2">
          Each key has one scope and reaches one project, while your role there allows the scope.
          The secret is shown once, when you create the key.
        </p>

        <form onSubmit={create} className="mt-4 flex flex-wrap items-end gap-2.5">
          <label>
            <span className="legend mb-1.5 block text-ink-3">Scope</span>
            <select
              value={scope}
              onChange={(e) => setScope(e.target.value as KeyScope)}
              className="field"
              aria-label="Scope"
            >
              {ALL_KEY_SCOPE.map((s) => (
                <option key={s} value={s}>
                  {s}: {SCOPE_TEXT[s]}
                </option>
              ))}
            </select>
          </label>
          {projects.length > 1 && (
            <label>
              <span className="legend mb-1.5 block text-ink-3">Project</span>
              <select value={keyProject} onChange={(e) => setKeyProject(e.target.value)} className="field" aria-label="Project">
                <option value="">your own project</option>
                {projects.map((p) => (
                  <option key={p.project} value={p.project}>
                    {p.label}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="min-w-[min(14rem,100%)] flex-1">
            <span className="legend mb-1.5 block text-ink-3">Name</span>
            <input
              value={name}
              maxLength={100}
              onChange={(e) => setName(e.target.value)}
              placeholder="checkout-service"
              className="well w-full px-3 py-2 text-[0.875rem] text-ink outline-none"
            />
          </label>
          <button
            type="submit"
            disabled={busy}
            className="legend panel-raised px-3 py-2.5 text-ink transition-colors hover:bg-panel-3 disabled:opacity-60"
          >
            Create key
          </button>
        </form>

        {fresh && <FreshKey created={fresh} onDone={() => setFresh(null)} />}

        <div className="scroll-x mt-5">
          {keys === null ? (
            <p className="legend text-ink-3">Reading</p>
          ) : keys.length === 0 ? (
            <p className="text-[0.875rem] text-ink-2">No keys yet. Create an ingest key to start sending runs.</p>
          ) : (
            <table className="w-full text-left text-[0.875rem]">
              <thead>
                <tr className="border-b border-scribe">
                  <th className="legend py-2 pr-4 font-normal text-ink-3">Name</th>
                  <th className="legend py-2 pr-4 font-normal text-ink-3">Scope</th>
                  <th className="legend py-2 pr-4 font-normal text-ink-3">Project</th>
                  <th className="legend py-2 pr-4 font-normal text-ink-3">Starts with</th>
                  <th className="legend py-2 pr-4 font-normal text-ink-3">Created</th>
                  <th className="py-2" />
                </tr>
              </thead>
              <tbody>
                {keys.map((k) => (
                  <tr key={k.id} className="border-b border-scribe-2">
                    <td className="py-2 pr-4 text-ink">{k.name ?? <span className="text-ink-3">unnamed</span>}</td>
                    <td className="data py-2 pr-4 text-ink-2">{k.scope}</td>
                    <td className="data py-2 pr-4 text-ink-2">{k.project}</td>
                    <td className="data py-2 pr-4 text-ink-2">{k.prefix}</td>
                    <td className="data py-2 pr-4 text-ink-2">{new Date(k.created_at).toLocaleDateString()}</td>
                    <td className="py-2 text-right">
                      {confirming === k.id ? (
                        <span className="inline-flex gap-2">
                          <button
                            type="button"
                            onClick={() => revoke(k.id)}
                            className="legend panel-raised px-2.5 py-1.5 text-[0.5625rem] text-oxide transition-colors hover:bg-panel-3"
                          >
                            Revoke now
                          </button>
                          <button
                            type="button"
                            onClick={() => setConfirming(null)}
                            className="legend px-2 py-1.5 text-[0.5625rem] text-ink-3 hover:text-ink"
                          >
                            Keep
                          </button>
                        </span>
                      ) : (
                        <button
                          type="button"
                          onClick={() => setConfirming(k.id)}
                          className="legend panel-raised px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink"
                        >
                          Revoke
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <section className="mt-10">
        <h3 className="legend text-ink-3">Send your first run</h3>
        <p className="mt-2 max-w-[62ch] text-[0.875rem] leading-relaxed text-ink-2">
          Every SDK reads these three variables. Signals an ingest key sends are stored in its
          project, whatever project they name.
        </p>
        <Snippet text={snippet} label="SDK environment" />
        <p className="mt-5 max-w-[62ch] text-[0.875rem] leading-relaxed text-ink-2">
          Or point any OpenTelemetry exporter at the same endpoint:
        </p>
        <Snippet text={otlp} label="OpenTelemetry environment" />
      </section>
      </>)}
    </div>
  )
}

function FreshKey({ created, onDone }: { created: CreatedAccountKey; onDone: () => void }) {
  return (
    <div className="panel mt-4 p-4" role="status">
      <p className="text-[0.875rem] leading-relaxed text-ink">
        Your new {created.key.scope} key. Copy it now: it is not shown again.
      </p>
      <Snippet text={created.secret} label="Key secret" />
      <button
        type="button"
        onClick={onDone}
        className="legend mt-3 px-0 py-1 text-[0.5625rem] text-ink-3 hover:text-ink"
      >
        I have copied it
      </button>
    </div>
  )
}

function Snippet({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      setCopied(false)
    }
  }
  return (
    <div className="mt-3 flex items-start gap-2">
      <pre
        aria-label={label}
        className="well scroll-x data min-w-0 flex-1 overflow-auto rounded-[2px] p-3 text-[0.8125rem] leading-relaxed text-ink"
      >
        {text}
      </pre>
      <button
        type="button"
        onClick={copy}
        className="legend panel-raised shrink-0 px-2.5 py-2 text-[0.5625rem] text-ink transition-colors hover:bg-panel-3"
      >
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  )
}
