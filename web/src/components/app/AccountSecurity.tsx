/**
 * The account's own security: password, address, two-step log-in, sessions,
 * and deleting the account.
 */

import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { accountCall, clearSession, seg } from '@/lib/api'
import type {
  Account,
  AccountDeletion,
  AccountNotice,
  DeleteResult,
  SessionInfo,
  TotpEnabled,
  TotpSetup,
} from '@/generated/types'
import { Alert, Field, failureText, MIN_PASSWORD } from './AccountAuth'

function Section({ title, children, lede }: { title: string; lede?: string; children: React.ReactNode }) {
  return (
    <section className="mt-9">
      <h3 className="legend text-ink-3">{title}</h3>
      {lede && <p className="mt-2 max-w-[62ch] text-[0.875rem] leading-relaxed text-ink-2">{lede}</p>}
      {children}
    </section>
  )
}

function Button({ children, busy, danger, type = 'submit', onClick }: {
  children: React.ReactNode
  busy?: boolean
  danger?: boolean
  type?: 'submit' | 'button'
  onClick?: () => void
}) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={busy}
      className={`legend panel-raised mt-4 px-3 py-2.5 transition-colors hover:bg-panel-3 disabled:opacity-60 ${danger ? 'text-oxide' : 'text-ink'}`}
    >
      {busy ? 'Working' : children}
    </button>
  )
}

/** A form that posts and reports its answer. */
function useAction(endpoint: string) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const run = async <T,>(work: () => Promise<T>, ok?: (t: T) => string | null) => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const t = await work()
      setNotice(ok ? ok(t) : null)
      return t
    } catch (e) {
      setError(failureText(e))
      return undefined
    } finally {
      setBusy(false)
    }
  }
  return { busy, error, notice, run, o: { endpoint } }
}

export function SecurityPanel({
  endpoint,
  account,
  onChanged,
  onDeleted,
}: {
  endpoint: string
  account: Account
  onChanged: () => void
  onDeleted: () => void
}) {
  return (
    <div>
      <PasswordForm endpoint={endpoint} account={account} />
      {!account.managed_by && <EmailForm endpoint={endpoint} account={account} />}
      <TwoStep endpoint={endpoint} account={account} onChanged={onChanged} />
      <Sessions endpoint={endpoint} />
      <DeleteAccount endpoint={endpoint} account={account} onDeleted={onDeleted} />
    </div>
  )
}

function PasswordForm({ endpoint, account }: { endpoint: string; account: Account }) {
  const a = useAction(endpoint)
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    void a.run(
      () => accountCall<AccountNotice>('POST', '/v1/api/account/password', a.o, {
        current_password: account.has_password ? current : undefined,
        new_password: next,
      }),
      (n) => {
        setCurrent('')
        setNext('')
        return n.message
      },
    )
  }
  return (
    <Section title={account.has_password ? 'Password' : 'Set a password'} lede="Changing it ends every other session of the account.">
      <form onSubmit={submit} className="panel mt-4 max-w-[28rem] p-5" noValidate>
        {account.has_password && <Field label="Current password" type="password" autoComplete="current-password" value={current} onChange={setCurrent} />}
        <Field label="New password" type="password" autoComplete="new-password" value={next} onChange={setNext} hint={`At least ${MIN_PASSWORD} characters.`} />
        <Alert text={a.error} />
        <Alert text={a.notice} tone="note" />
        <Button busy={a.busy}>Change password</Button>
      </form>
    </Section>
  )
}

function EmailForm({ endpoint, account }: { endpoint: string; account: Account }) {
  const a = useAction(endpoint)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    void a.run(
      () => accountCall<AccountNotice>('POST', '/v1/api/account/email', a.o, {
        email,
        password: account.has_password ? password : undefined,
      }),
      (n) => n.message,
    )
  }
  return (
    <Section title="Email address" lede={`Now ${account.email}${account.email_verified ? ', confirmed' : ', not confirmed yet'}. A new address changes when you open the link we send to it.`}>
      <form onSubmit={submit} className="panel mt-4 max-w-[28rem] p-5" noValidate>
        <Field label="New address" type="email" autoComplete="email" value={email} onChange={setEmail} />
        {account.has_password && <Field label="Password" type="password" autoComplete="current-password" value={password} onChange={setPassword} />}
        <Alert text={a.error} />
        <Alert text={a.notice} tone="note" />
        <Button busy={a.busy}>Send link</Button>
      </form>
    </Section>
  )
}

function TwoStep({ endpoint, account, onChanged }: { endpoint: string; account: Account; onChanged: () => void }) {
  const a = useAction(endpoint)
  const [setup, setSetup] = useState<TotpSetup | null>(null)
  const [codes, setCodes] = useState<string[] | null>(null)
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')

  if (codes) {
    return (
      <Section title="Two-step log-in" lede="It is on. Keep these recovery codes somewhere safe: each logs in once without the app, and they are not shown again.">
        <pre className="well data mt-4 max-w-[28rem] p-3 text-[0.875rem] leading-relaxed text-ink">{codes.join('\n')}</pre>
        <Button type="button" onClick={() => setCodes(null)}>I have saved them</Button>
      </Section>
    )
  }
  if (account.totp_enabled) {
    const off = (e: FormEvent) => {
      e.preventDefault()
      void a.run(
        () => accountCall<AccountNotice>('POST', '/v1/api/account/totp/disable', a.o, {
          code,
          password: account.has_password ? password : undefined,
        }),
        (n) => {
          onChanged()
          return n.message
        },
      )
    }
    return (
      <Section title="Two-step log-in" lede="On. Log-in, password reset and deleting the account ask for a code.">
        <form onSubmit={off} className="panel mt-4 max-w-[28rem] p-5" noValidate>
          {account.has_password && <Field label="Password" type="password" autoComplete="current-password" value={password} onChange={setPassword} />}
          <Field label="Code, or a recovery code" inputMode="numeric" autoComplete="one-time-code" value={code} onChange={setCode} />
          <Alert text={a.error} />
          <Button busy={a.busy} danger>Turn off</Button>
        </form>
      </Section>
    )
  }
  if (setup) {
    const enable = (e: FormEvent) => {
      e.preventDefault()
      void a.run(
        () => accountCall<TotpEnabled>('POST', '/v1/api/account/totp/enable', a.o, { code }),
        (t) => {
          setCodes(t.recovery_codes)
          setSetup(null)
          setCode('')
          onChanged()
          return null
        },
      )
    }
    return (
      <Section title="Two-step log-in" lede="Scan the code with an authenticator app, or type the secret into it, then enter the code it shows.">
        <div className="panel mt-4 max-w-[28rem] p-5">
          {/* Rendered by the daemon from the secret, with its own light background, as scanners expect. */}
          <div className="mx-auto w-[12rem]" dangerouslySetInnerHTML={{ __html: setup.qr_svg }} />
          <p className="data mt-4 break-all text-[0.8125rem] text-ink-2">{setup.secret}</p>
          <form onSubmit={enable} noValidate className="mt-2">
            <Field label="Code from the app" inputMode="numeric" autoComplete="one-time-code" value={code} onChange={setCode} />
            <Alert text={a.error} />
            <Button busy={a.busy}>Turn on</Button>
          </form>
        </div>
      </Section>
    )
  }
  return (
    <Section title="Two-step log-in" lede="Off. With it on, logging in needs a code from an authenticator app as well as the password.">
      <Alert text={a.error} />
      <Button type="button" busy={a.busy} onClick={() => void a.run(() => accountCall<TotpSetup>('POST', '/v1/api/account/totp/setup', a.o)).then((s) => s && setSetup(s))}>
        Set up
      </Button>
    </Section>
  )
}

function Sessions({ endpoint }: { endpoint: string }) {
  const [rows, setRows] = useState<SessionInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const load = useCallback(() => {
    accountCall<SessionInfo[]>('GET', '/v1/api/account/sessions', { endpoint })
      .then(setRows)
      .catch((e) => setError(failureText(e)))
  }, [endpoint])
  useEffect(load, [load])
  const end = async (id: string) => {
    try {
      await accountCall<DeleteResult>('DELETE', `/v1/api/account/sessions/${seg(id)}`, { endpoint })
      load()
    } catch (e) {
      setError(failureText(e))
    }
  }
  return (
    <Section title="Sessions" lede="Every browser and client logged in as this account.">
      <Alert text={error} />
      <div className="scroll-x mt-4">
        {rows === null ? (
          <p className="legend text-ink-3">Reading</p>
        ) : (
          <table className="w-full text-left text-[0.875rem]">
            <thead>
              <tr className="border-b border-scribe">
                <th className="legend py-2 pr-4 font-normal text-ink-3">Client</th>
                <th className="legend py-2 pr-4 font-normal text-ink-3">How</th>
                <th className="legend py-2 pr-4 font-normal text-ink-3">Address</th>
                <th className="legend py-2 pr-4 font-normal text-ink-3">Started</th>
                <th className="py-2" />
              </tr>
            </thead>
            <tbody>
              {rows.map((s) => (
                <tr key={s.id} className="border-b border-scribe-2">
                  <td className="max-w-[18rem] truncate py-2 pr-4 text-ink" title={s.user_agent ?? ''}>
                    {s.user_agent ?? <span className="text-ink-3">unknown</span>}
                  </td>
                  <td className="data py-2 pr-4 text-ink-2">{s.via}</td>
                  <td className="data py-2 pr-4 text-ink-2">{s.address ?? ''}</td>
                  <td className="data py-2 pr-4 text-ink-2">{new Date(s.created_at).toLocaleString()}</td>
                  <td className="py-2 text-right">
                    {s.current ? (
                      <span className="legend text-[0.5625rem] text-ink-3">This one</span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void end(s.id)}
                        className="legend panel-raised px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink"
                      >
                        End
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </Section>
  )
}

function DeleteAccount({ endpoint, account, onDeleted }: { endpoint: string; account: Account; onDeleted: () => void }) {
  const a = useAction(endpoint)
  const [open, setOpen] = useState(false)
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    void a.run(() => accountCall<AccountDeletion>('POST', '/v1/api/account/delete', a.o, {
      password: account.has_password ? password : undefined,
      totp_code: account.totp_enabled ? code : undefined,
    })).then((d) => {
      if (d) {
        clearSession()
        onDeleted()
      }
    })
  }
  return (
    <Section
      title="Delete the account"
      lede="The account, its keys and its sessions go. Every organisation it is the only member of goes too, with the stored runs of its projects. An organisation with other members needs another owner first."
    >
      {!open ? (
        <Button type="button" danger onClick={() => setOpen(true)}>Delete account</Button>
      ) : (
        <form onSubmit={submit} className="panel mt-4 max-w-[28rem] p-5" noValidate>
          {account.has_password && <Field label="Password" type="password" autoComplete="current-password" value={password} onChange={setPassword} />}
          {account.totp_enabled && <Field label="Code from your authenticator app" inputMode="numeric" value={code} onChange={setCode} />}
          <Alert text={a.error} />
          <div className="flex gap-3">
            <Button busy={a.busy} danger>Delete for good</Button>
            <Button type="button" onClick={() => setOpen(false)}>Keep it</Button>
          </div>
        </form>
      )}
    </Section>
  )
}
