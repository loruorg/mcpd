/**
 * Organisations: the switcher, members and invitations, roles, projects,
 * keys, single sign-on, SCIM, usage and the audit log.
 *
 * What a reader sees follows their role: every member sees the organisation,
 * its members and its usage; admins and owners see the rest. The daemon
 * enforces the same rule on every door, so hiding a control here is
 * convenience, not protection.
 */

import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { accountCall, seg } from '@/lib/api'
import type {
  AccountKey,
  AuditEvent,
  CreatedScimToken,
  DeleteResult,
  GroupRole,
  Org,
  OrgInvite,
  OrgMember,
  OrgProject,
  OrgRole,
  OrgUsage,
  ProjectRole,
  ScimTokenInfo,
  SsoConfig,
  SsoProvider,
} from '@/generated/types'
import { ALL_ORG_ROLE, ALL_PROJECT_ROLE } from '@/generated/types'
import { Alert, failureText } from './AccountAuth'

const ROLE_TEXT: Record<OrgRole, string> = {
  owner: 'everything, including owners and quotas',
  admin: 'every scope, and manages members, projects, SSO and SCIM',
  member: 'every scope but admin',
  viewer: 'reads only',
}

function Section({ title, lede, children }: { title: string; lede?: string; children: ReactNode }) {
  return (
    <section className="mt-9">
      <h3 className="legend text-ink-3">{title}</h3>
      {lede && <p className="mt-2 max-w-[62ch] text-[0.875rem] leading-relaxed text-ink-2">{lede}</p>}
      {children}
    </section>
  )
}

const btn = 'legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3 disabled:opacity-60'
const small = 'legend panel-raised px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink'
const input = 'well w-full px-3 py-2 text-[0.875rem] text-ink outline-none'

function Th({ children }: { children?: ReactNode }) {
  return <th className="legend py-2 pr-4 font-normal text-ink-3">{children}</th>
}

/** The organisations panel: a switcher and the chosen organisation. */
export function OrgsPanel({ endpoint }: { endpoint: string }) {
  const o = { endpoint }
  const [orgs, setOrgs] = useState<Org[] | null>(null)
  const [chosen, setChosen] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [name, setName] = useState('')

  const load = useCallback(async () => {
    try {
      const list = await accountCall<Org[]>('GET', '/v1/api/orgs', o)
      setOrgs(list)
      setChosen((c) => (c && list.some((x) => x.id === c) ? c : (list[0]?.id ?? null)))
    } catch (e) {
      setError(failureText(e))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endpoint])
  useEffect(() => {
    void load()
  }, [load])

  const create = async (e: FormEvent) => {
    e.preventDefault()
    try {
      const made = await accountCall<Org>('POST', '/v1/api/orgs', o, { name })
      setName('')
      await load()
      setChosen(made.id)
    } catch (err) {
      setError(failureText(err))
    }
  }

  const org = orgs?.find((x) => x.id === chosen) ?? null
  return (
    <div>
      <div className="mt-6 flex flex-wrap items-end gap-3">
        <label>
          <span className="legend mb-1.5 block text-ink-3">Organisation</span>
          <select className="field" value={chosen ?? ''} onChange={(e) => setChosen(e.target.value)} aria-label="Organisation">
            {(orgs ?? []).map((x) => (
              <option key={x.id} value={x.id}>
                {x.name} ({x.role})
              </option>
            ))}
          </select>
        </label>
        <form onSubmit={create} className="flex flex-wrap items-end gap-2">
          <label>
            <span className="legend mb-1.5 block text-ink-3">New organisation</span>
            <input value={name} maxLength={100} onChange={(e) => setName(e.target.value)} placeholder="Acme" className={input} />
          </label>
          <button type="submit" disabled={!name.trim()} className={btn}>
            Create
          </button>
        </form>
      </div>
      <Alert text={error} />
      {org && <OrgView key={org.id} endpoint={endpoint} org={org} onChanged={load} />}
    </div>
  )
}

function OrgView({ endpoint, org, onChanged }: { endpoint: string; org: Org; onChanged: () => void }) {
  const manages = org.role === 'owner' || org.role === 'admin'
  const base = `/v1/api/orgs/${seg(org.id)}`
  return (
    <div>
      <Usage endpoint={endpoint} base={base} org={org} onChanged={onChanged} />
      <Projects endpoint={endpoint} base={base} manages={manages} />
      <Members endpoint={endpoint} base={base} org={org} manages={manages} />
      {manages && <Invites endpoint={endpoint} base={base} org={org} />}
      {manages && <OrgKeys endpoint={endpoint} base={base} />}
      {manages && <Sso endpoint={endpoint} base={base} />}
      {manages && <GroupRoles endpoint={endpoint} base={base} />}
      {manages && <Scim endpoint={endpoint} base={base} />}
      {manages && <Audit endpoint={endpoint} base={base} />}
    </div>
  )
}

function useList<T>(endpoint: string, path: string) {
  const [rows, setRows] = useState<T[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const load = useCallback(() => {
    accountCall<T[]>('GET', path, { endpoint })
      .then((r) => {
        setRows(r)
        setError(null)
      })
      .catch((e) => setError(failureText(e)))
  }, [endpoint, path])
  useEffect(load, [load])
  const act = async (work: () => Promise<unknown>) => {
    try {
      await work()
      setError(null)
      load()
    } catch (e) {
      setError(failureText(e))
    }
  }
  return { rows, error, load, act, setError }
}

function Usage({ endpoint, base, org, onChanged }: { endpoint: string; base: string; org: Org; onChanged: () => void }) {
  const [u, setU] = useState<OrgUsage | null>(null)
  const [quota, setQuota] = useState('')
  const [days, setDays] = useState('')
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    accountCall<OrgUsage>('GET', `${base}/usage`, { endpoint }).then(setU).catch((e) => setError(failureText(e)))
  }, [endpoint, base])
  const save = async (e: FormEvent) => {
    e.preventDefault()
    try {
      await accountCall<Org>('PATCH', base, { endpoint }, {
        quota_per_minute: quota ? Number(quota) : undefined,
        retention_days: days ? Number(days) : undefined,
      })
      setQuota('')
      setDays('')
      onChanged()
    } catch (err) {
      setError(failureText(err))
    }
  }
  const cell = (label: string, value: ReactNode) => (
    <div className="min-w-0">
      <dt className="legend text-ink-3">{label}</dt>
      <dd className="data mt-1 truncate text-[0.875rem] text-ink">{value}</dd>
    </div>
  )
  return (
    <Section title="Usage" lede="Every project of the organisation shares one ceiling on signals per minute. Counted since the daemon started.">
      <dl className="panel mt-4 grid gap-x-8 gap-y-4 p-5 sm:grid-cols-3">
        {cell('Ceiling', u?.quota_per_minute ? `${u.quota_per_minute.toLocaleString()} a minute` : 'none')}
        {cell('This minute', u ? u.admitted_this_minute.toLocaleString() : '')}
        {cell('Stored', u ? u.admitted_total.toLocaleString() : '')}
        {cell('Refused for the ceiling', u ? u.refused_total.toLocaleString() : '')}
        {cell('Retention', u?.retention_days ? `${u.retention_days} days` : 'forever')}
        {cell('Members, projects, keys', u ? `${u.members}, ${u.projects}, ${u.keys}` : '')}
      </dl>
      {org.role === 'owner' && (
        <form onSubmit={save} className="mt-4 flex flex-wrap items-end gap-2">
          <label>
            <span className="legend mb-1.5 block text-ink-3">Ceiling a minute</span>
            <input value={quota} inputMode="numeric" onChange={(e) => setQuota(e.target.value.replace(/\D/g, ''))} placeholder="0 restores the default" className={input} />
          </label>
          <label>
            <span className="legend mb-1.5 block text-ink-3">Retention, days</span>
            <input value={days} inputMode="numeric" onChange={(e) => setDays(e.target.value.replace(/\D/g, ''))} placeholder="0 restores the default" className={input} />
          </label>
          <button type="submit" disabled={!quota && !days} className={btn}>
            Save
          </button>
        </form>
      )}
      <Alert text={error} />
    </Section>
  )
}

function Projects({ endpoint, base, manages }: { endpoint: string; base: string; manages: boolean }) {
  const l = useList<OrgProject>(endpoint, `${base}/projects`)
  const [label, setLabel] = useState('')
  const [confirm, setConfirm] = useState<string | null>(null)
  return (
    <Section title="Projects" lede="Keys and reads name one of these. Deleting a project erases its stored runs.">
      <Alert text={l.error} />
      <ul className="mt-4 divide-y divide-scribe-2 border-y border-scribe-2">
        {(l.rows ?? []).map((p) => (
          <li key={p.project} className="flex flex-wrap items-center justify-between gap-3 py-2 text-[0.875rem]">
            <span>
              <span className="data text-ink">{p.project}</span>
              {p.label && <span className="ml-3 text-ink-2">{p.label}</span>}
            </span>
            {manages &&
              (confirm === p.project ? (
                <span className="inline-flex gap-2">
                  <button type="button" className={`${small} text-oxide`} onClick={() => void l.act(() => accountCall<unknown>('DELETE', `${base}/projects/${seg(p.project)}`, { endpoint }))}>
                    Erase now
                  </button>
                  <button type="button" className={small} onClick={() => setConfirm(null)}>
                    Keep
                  </button>
                </span>
              ) : (
                <button type="button" className={small} onClick={() => setConfirm(p.project)}>
                  Delete
                </button>
              ))}
          </li>
        ))}
      </ul>
      {manages && (
        <form
          className="mt-4 flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            void l.act(() => accountCall<OrgProject>('POST', `${base}/projects`, { endpoint }, { label: label || undefined })).then(() => setLabel(''))
          }}
        >
          <label>
            <span className="legend mb-1.5 block text-ink-3">Label</span>
            <input value={label} maxLength={100} onChange={(e) => setLabel(e.target.value)} placeholder="staging" className={input} />
          </label>
          <button type="submit" className={btn}>
            Create project
          </button>
        </form>
      )}
    </Section>
  )
}

function Members({ endpoint, base, org, manages }: { endpoint: string; base: string; org: Org; manages: boolean }) {
  const l = useList<OrgMember>(endpoint, `${base}/members`)
  const projects = useList<OrgProject>(endpoint, `${base}/projects`)
  const [scoped, setScoped] = useState<string | null>(null)
  const [project, setProject] = useState('')
  const [prole, setProle] = useState<ProjectRole | ''>('')
  const roles = org.role === 'owner' ? ALL_ORG_ROLE : ALL_ORG_ROLE.filter((r) => r !== 'owner')
  return (
    <Section title="Members" lede="A role maps onto the key scopes on every project. A project role replaces a viewer's or member's role on one project.">
      <Alert text={l.error} />
      <div className="scroll-x mt-4">
        <table className="w-full text-left text-[0.875rem]">
          <thead>
            <tr className="border-b border-scribe">
              <Th>Email</Th>
              <Th>Role</Th>
              <Th>Project roles</Th>
              <Th>Joined</Th>
              <Th />
            </tr>
          </thead>
          <tbody>
            {(l.rows ?? []).map((m) => {
              const locked = !manages || (m.role === 'owner' && org.role !== 'owner')
              return (
                <tr key={m.account_id} className="border-b border-scribe-2 align-top">
                  <td className="py-2 pr-4 text-ink">
                    {m.email}
                    <span className="data ml-2 text-[0.75rem] text-ink-3">
                      {m.via}
                      {m.totp_enabled ? ', 2-step' : ''}
                    </span>
                  </td>
                  <td className="py-2 pr-4">
                    {locked ? (
                      <span className="data text-ink-2">{m.role}</span>
                    ) : (
                      <select
                        className="field"
                        aria-label={`Role of ${m.email}`}
                        value={m.role}
                        onChange={(e) => void l.act(() => accountCall<OrgMember>('PATCH', `${base}/members/${seg(m.account_id)}`, { endpoint }, { role: e.target.value }))}
                      >
                        {roles.map((r) => (
                          <option key={r} value={r} title={ROLE_TEXT[r]}>
                            {r}
                          </option>
                        ))}
                      </select>
                    )}
                  </td>
                  <td className="data py-2 pr-4 text-ink-2">
                    {m.project_roles.map((g) => `${g.project} ${g.role}`).join(', ') || 'none'}
                    {manages && scoped === m.account_id && (
                      <form
                        className="mt-2 flex flex-wrap gap-2"
                        onSubmit={(e) => {
                          e.preventDefault()
                          void l
                            .act(() => accountCall<OrgMember>('POST', `${base}/members/${seg(m.account_id)}/project-roles`, { endpoint }, { project, role: prole || undefined }))
                            .then(() => setScoped(null))
                        }}
                      >
                        <select className="field" aria-label="Project" value={project} onChange={(e) => setProject(e.target.value)}>
                          <option value="">project</option>
                          {(projects.rows ?? []).map((p) => (
                            <option key={p.project} value={p.project}>
                              {p.label ?? p.project}
                            </option>
                          ))}
                        </select>
                        <select className="field" aria-label="Project role" value={prole} onChange={(e) => setProle(e.target.value as ProjectRole | '')}>
                          <option value="">organisation role</option>
                          {ALL_PROJECT_ROLE.map((r) => (
                            <option key={r} value={r}>
                              {r}
                            </option>
                          ))}
                        </select>
                        <button type="submit" disabled={!project} className={small}>
                          Set
                        </button>
                      </form>
                    )}
                  </td>
                  <td className="data py-2 pr-4 text-ink-2">{new Date(m.joined_at).toLocaleDateString()}</td>
                  <td className="py-2 text-right">
                    <span className="inline-flex gap-2">
                      {manages && m.role !== 'owner' && m.role !== 'admin' && (
                        <button type="button" className={small} onClick={() => setScoped(scoped === m.account_id ? null : m.account_id)}>
                          Project role
                        </button>
                      )}
                      {!locked && (
                        <button
                          type="button"
                          className={small}
                          onClick={() => void l.act(() => accountCall<DeleteResult>('DELETE', `${base}/members/${seg(m.account_id)}`, { endpoint }))}
                        >
                          Remove
                        </button>
                      )}
                    </span>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </Section>
  )
}

function Invites({ endpoint, base, org }: { endpoint: string; base: string; org: Org }) {
  const l = useList<OrgInvite>(endpoint, `${base}/invites`)
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<OrgRole>('member')
  const roles = org.role === 'owner' ? ALL_ORG_ROLE : ALL_ORG_ROLE.filter((r) => r !== 'owner')
  return (
    <Section title="Invitations" lede="The link we mail works once, for seven days.">
      <form
        className="mt-4 flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          void l.act(() => accountCall<OrgInvite>('POST', `${base}/invites`, { endpoint }, { email, role })).then(() => setEmail(''))
        }}
      >
        <label className="min-w-[min(16rem,100%)] flex-1">
          <span className="legend mb-1.5 block text-ink-3">Email</span>
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="kim@example.com" className={input} />
        </label>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Role</span>
          <select className="field" value={role} onChange={(e) => setRole(e.target.value as OrgRole)} aria-label="Role">
            {roles.map((r) => (
              <option key={r} value={r}>
                {r}: {ROLE_TEXT[r]}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" disabled={!email} className={btn}>
          Invite
        </button>
      </form>
      <Alert text={l.error} />
      {(l.rows ?? []).length > 0 && (
        <ul className="mt-4 divide-y divide-scribe-2 border-y border-scribe-2">
          {(l.rows ?? []).map((i) => (
            <li key={i.id} className="flex flex-wrap items-center justify-between gap-3 py-2 text-[0.875rem]">
              <span className="text-ink">
                {i.email} <span className="data text-ink-2">{i.role}</span>
                <span className="data ml-3 text-[0.75rem] text-ink-3">until {new Date(i.expires_at).toLocaleDateString()}</span>
              </span>
              <button type="button" className={small} onClick={() => void l.act(() => accountCall<DeleteResult>('DELETE', `${base}/invites/${seg(i.id)}`, { endpoint }))}>
                Revoke
              </button>
            </li>
          ))}
        </ul>
      )}
    </Section>
  )
}

function OrgKeys({ endpoint, base }: { endpoint: string; base: string }) {
  const l = useList<AccountKey>(endpoint, `${base}/keys`)
  return (
    <Section title="Every key in the organisation" lede="Keys any member made on these projects. Revoking one stops it at once.">
      <Alert text={l.error} />
      <div className="scroll-x mt-4">
        <table className="w-full text-left text-[0.875rem]">
          <thead>
            <tr className="border-b border-scribe">
              <Th>Name</Th>
              <Th>Scope</Th>
              <Th>Project</Th>
              <Th>Made by</Th>
              <Th />
            </tr>
          </thead>
          <tbody>
            {(l.rows ?? []).map((k) => (
              <tr key={k.id} className="border-b border-scribe-2">
                <td className="py-2 pr-4 text-ink">{k.name ?? <span className="text-ink-3">unnamed</span>}</td>
                <td className="data py-2 pr-4 text-ink-2">{k.scope}</td>
                <td className="data py-2 pr-4 text-ink-2">{k.project}</td>
                <td className="py-2 pr-4 text-ink-2">{k.created_by ?? ''}</td>
                <td className="py-2 text-right">
                  <button type="button" className={small} onClick={() => void l.act(() => accountCall<DeleteResult>('DELETE', `${base}/keys/${seg(k.id)}`, { endpoint }))}>
                    Revoke
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  )
}

function Sso({ endpoint, base }: { endpoint: string; base: string }) {
  const o = { endpoint }
  const [cfg, setCfg] = useState<SsoConfig | null>(null)
  const [provider, setProvider] = useState<SsoProvider>('oidc')
  const [issuer, setIssuer] = useState('')
  const [clientId, setClientId] = useState('')
  const [secret, setSecret] = useState('')
  const [domains, setDomains] = useState('')
  const [enforced, setEnforced] = useState(false)
  const [role, setRole] = useState<OrgRole>('member')
  const [groups, setGroups] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const fill = (c: SsoConfig) => {
    setCfg(c)
    setProvider(c.provider)
    setIssuer(c.issuer ?? '')
    setClientId(c.client_id)
    setDomains(c.domains.join(', '))
    setEnforced(c.enforced)
    setRole(c.default_role)
    setGroups(c.groups_claim ?? '')
  }
  useEffect(() => {
    accountCall<SsoConfig>('GET', `${base}/sso`, o)
      .then(fill)
      .catch(() => setCfg(null))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endpoint, base])
  const save = async (e: FormEvent) => {
    e.preventDefault()
    setError(null)
    setNotice(null)
    try {
      fill(
        await accountCall<SsoConfig>('PUT', `${base}/sso`, o, {
          provider,
          issuer: provider === 'oidc' ? issuer : undefined,
          client_id: clientId,
          client_secret: secret || undefined,
          domains: domains.split(/[\s,]+/).filter(Boolean),
          enforced,
          default_role: role,
          groups_claim: groups || undefined,
        }),
      )
      setSecret('')
      setNotice('Saved.')
    } catch (err) {
      setError(failureText(err))
    }
  }
  const remove = async () => {
    try {
      await accountCall<DeleteResult>('DELETE', `${base}/sso`, o)
      setCfg(null)
      setNotice('Single sign-on is off.')
    } catch (err) {
      setError(failureText(err))
    }
  }
  const locked = cfg?.managed_by_config ?? false
  return (
    <Section
      title="Single sign-on"
      lede="Any OpenID Connect provider, such as Google, Microsoft Entra ID, Okta or Auth0, or GitHub through an OAuth app. Register the redirect URL below with the provider."
    >
      {cfg && <p className="data mt-3 break-all text-[0.8125rem] text-ink-2">Redirect URL: {cfg.redirect_url}</p>}
      {locked && <p className="mt-3 text-[0.875rem] text-ink-2">The daemon's configuration file sets these.</p>}
      <form onSubmit={save} className="panel mt-4 grid max-w-[40rem] gap-3 p-5 sm:grid-cols-2" noValidate>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Provider</span>
          <select className="field" disabled={locked} value={provider} onChange={(e) => setProvider(e.target.value as SsoProvider)} aria-label="Provider">
            <option value="oidc">OpenID Connect</option>
            <option value="github">GitHub</option>
          </select>
        </label>
        {provider === 'oidc' && (
          <label>
            <span className="legend mb-1.5 block text-ink-3">Issuer URL</span>
            <input disabled={locked} value={issuer} onChange={(e) => setIssuer(e.target.value)} placeholder="https://accounts.google.com" className={input} />
          </label>
        )}
        <label>
          <span className="legend mb-1.5 block text-ink-3">Client id</span>
          <input disabled={locked} value={clientId} onChange={(e) => setClientId(e.target.value)} className={input} />
        </label>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Client secret</span>
          <input disabled={locked} type="password" autoComplete="off" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={cfg?.has_client_secret ? 'stored; leave empty to keep' : ''} className={input} />
        </label>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Email domains</span>
          <input disabled={locked} value={domains} onChange={(e) => setDomains(e.target.value)} placeholder="example.com" className={input} />
        </label>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Role for new members</span>
          <select className="field" disabled={locked} value={role} onChange={(e) => setRole(e.target.value as OrgRole)} aria-label="Default role">
            {ALL_ORG_ROLE.filter((r) => r !== 'owner').map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="legend mb-1.5 block text-ink-3">Groups claim</span>
          <input disabled={locked} value={groups} onChange={(e) => setGroups(e.target.value)} placeholder="groups" className={input} />
        </label>
        <label className="flex items-center gap-2 self-end text-[0.875rem] text-ink">
          <input type="checkbox" disabled={locked} checked={enforced} onChange={(e) => setEnforced(e.target.checked)} />
          Members in these domains must use SSO
        </label>
        {!locked && (
          <div className="flex gap-2 sm:col-span-2">
            <button type="submit" disabled={!clientId} className={btn}>
              Save
            </button>
            {cfg && (
              <button type="button" onClick={() => void remove()} className={`${btn} text-oxide`}>
                Remove
              </button>
            )}
          </div>
        )}
      </form>
      <Alert text={error} />
      <Alert text={notice} tone="note" />
    </Section>
  )
}

function GroupRoles({ endpoint, base }: { endpoint: string; base: string }) {
  const l = useList<GroupRole>(endpoint, `${base}/group-roles`)
  const [group, setGroup] = useState('')
  const [role, setRole] = useState<OrgRole>('member')
  const put = (rows: GroupRole[]) => l.act(() => accountCall<GroupRole[]>('PUT', `${base}/group-roles`, { endpoint }, rows))
  return (
    <Section title="Group roles" lede="A member single sign-on or SCIM manages gets the highest role among their groups. A group grants admin at most.">
      <Alert text={l.error} />
      <ul className="mt-4 divide-y divide-scribe-2 border-y border-scribe-2">
        {(l.rows ?? []).map((g) => (
          <li key={g.group} className="flex items-center justify-between gap-3 py-2 text-[0.875rem]">
            <span className="text-ink">
              {g.group} <span className="data text-ink-2">{g.role}</span>
            </span>
            <button type="button" className={small} onClick={() => void put((l.rows ?? []).filter((x) => x.group !== g.group))}>
              Remove
            </button>
          </li>
        ))}
      </ul>
      <form
        className="mt-4 flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          void put([...(l.rows ?? []).filter((x) => x.group !== group.toLowerCase()), { group, role }]).then(() => setGroup(''))
        }}
      >
        <label>
          <span className="legend mb-1.5 block text-ink-3">Group</span>
          <input value={group} onChange={(e) => setGroup(e.target.value)} placeholder="mcpd-admins" className={input} />
        </label>
        <select className="field" value={role} onChange={(e) => setRole(e.target.value as OrgRole)} aria-label="Group role">
          {ALL_ORG_ROLE.filter((r) => r !== 'owner').map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button type="submit" disabled={!group.trim()} className={btn}>
          Map
        </button>
      </form>
    </Section>
  )
}

function Scim({ endpoint, base }: { endpoint: string; base: string }) {
  const o = { endpoint }
  const [info, setInfo] = useState<ScimTokenInfo | null>(null)
  const [fresh, setFresh] = useState<CreatedScimToken | null>(null)
  const [error, setError] = useState<string | null>(null)
  const load = useCallback(() => {
    accountCall<ScimTokenInfo>('GET', `${base}/scim-token`, o).then(setInfo).catch((e) => setError(failureText(e)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endpoint, base])
  useEffect(load, [load])
  const rotate = async () => {
    try {
      setFresh(await accountCall<CreatedScimToken>('POST', `${base}/scim-token`, o))
      load()
    } catch (e) {
      setError(failureText(e))
    }
  }
  const revoke = async () => {
    try {
      await accountCall<DeleteResult>('DELETE', `${base}/scim-token`, o)
      setFresh(null)
      load()
    } catch (e) {
      setError(failureText(e))
    }
  }
  return (
    <Section title="SCIM provisioning" lede="Give your identity provider the base URL and a token. A new token replaces the old one. Deprovisioning a user ends their access and their sessions.">
      {info && <p className="data mt-3 break-all text-[0.8125rem] text-ink-2">Base URL: {info.base_url}</p>}
      {info?.configured && (
        <p className="mt-2 text-[0.875rem] text-ink-2">
          A token starting <span className="data">{info.prefix}</span> is in use.
        </p>
      )}
      {fresh && (
        <div className="panel mt-4 p-4" role="status">
          <p className="text-[0.875rem] text-ink">Copy the token now: it is not shown again.</p>
          <pre className="well data mt-2 overflow-auto p-3 text-[0.8125rem] text-ink">{fresh.token}</pre>
        </div>
      )}
      <div className="mt-4 flex gap-2">
        <button type="button" className={btn} onClick={() => void rotate()}>
          {info?.configured ? 'Replace token' : 'Create token'}
        </button>
        {info?.configured && (
          <button type="button" className={`${btn} text-oxide`} onClick={() => void revoke()}>
            Revoke
          </button>
        )}
      </div>
      <Alert text={error} />
    </Section>
  )
}

function Audit({ endpoint, base }: { endpoint: string; base: string }) {
  const l = useList<AuditEvent>(endpoint, `${base}/audit`)
  return (
    <Section title="Audit log" lede="Membership, role, project, single sign-on and SCIM changes, newest first, refusals included.">
      <Alert text={l.error} />
      <div className="scroll-x mt-4">
        <table className="w-full text-left text-[0.875rem]">
          <thead>
            <tr className="border-b border-scribe">
              <Th>When</Th>
              <Th>Who</Th>
              <Th>Action</Th>
              <Th>On</Th>
              <Th>Result</Th>
            </tr>
          </thead>
          <tbody>
            {(l.rows ?? []).slice(0, 100).map((r) => (
              <tr key={r.audit_id} className="border-b border-scribe-2">
                <td className="data py-2 pr-4 text-ink-2">{new Date(r.timestamp).toLocaleString()}</td>
                <td className="py-2 pr-4 text-ink">{r.subject || r.principal}</td>
                <td className="data py-2 pr-4 text-ink">{r.action}</td>
                <td className="data py-2 pr-4 text-ink-2">{r.resource ?? ''}</td>
                <td className={`data py-2 pr-4 ${r.result === 'allowed' ? 'text-ink-2' : 'text-oxide'}`}>{r.result}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  )
}
