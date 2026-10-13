/**
 * The screens before a session: log in, sign up, forgot password, log in with
 * single sign-on, and the pages a mailed link opens.
 *
 * Mailed links carry their token in the URL fragment (`#verify=…`,
 * `#reset=…`, `#email=…`, `#invite=…`), which never reaches a server log or a
 * `Referer` header. The single sign-on provider sends the browser back with
 * `?code=…&state=…`; the binding secret that ties that callback to this
 * browser was kept in session storage when the login started.
 */

import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { accountCall, api, DaemonError, DaemonUnreachable, saveSession } from '@/lib/api'
import type {
  Account,
  AccountNotice,
  AccountSession,
  InviteAccepted,
  InviteLookup,
  SsoStart,
} from '@/generated/types'

export const MIN_PASSWORD = 10
const SSO_BINDING = 'mcpd:sso-binding'
/** An invitation opened while logged out, accepted after logging in. */
export const PENDING_INVITE = 'mcpd:pending-invite'

export function failureText(e: unknown): string {
  if (e instanceof DaemonUnreachable) return `Nothing answered at ${e.endpoint}.`
  if (e instanceof DaemonError) return e.message
  return e instanceof Error ? e.message : String(e)
}

export function errorCode(e: unknown): string | undefined {
  return e instanceof DaemonError ? e.body.error : undefined
}

/** A link the reader opened: which kind, and its token. */
export interface LinkToken {
  kind: 'verify' | 'reset' | 'email' | 'invite' | 'forgot'
  token: string
}

function parseLink(hash: string): LinkToken | null {
  const m = /^#(verify|reset|email|invite|forgot)(?:=([A-Za-z0-9_-]+))?$/.exec(hash)
  if (!m) return null
  return { kind: m[1] as LinkToken['kind'], token: m[2] ?? '' }
}

/* Read once, when the page loads. Screens rewrite the query string as they
   settle, and a rewrite drops the fragment before the screen that reads it has
   mounted. */
let loadedLink: LinkToken | null = typeof window === 'undefined' ? null : parseLink(location.hash)

/** The mailed link the page was opened with, if any. */
export function linkFromUrl(): LinkToken | null {
  if (typeof window === 'undefined') return null
  return parseLink(location.hash) ?? loadedLink
}

/** Remove the fragment and the single sign-on query from the URL. */
export function clearLink() {
  loadedLink = null
  const p = new URLSearchParams(location.search)
  for (const k of ['code', 'state', 'error', 'error_description', 'session_state', 'iss']) p.delete(k)
  const q = p.toString()
  history.replaceState(history.state, '', `${location.pathname}${q ? `?${q}` : ''}`)
}

function ssoCallbackFromUrl(): { code: string; state: string } | { error: string } | null {
  if (typeof window === 'undefined') return null
  const p = new URLSearchParams(location.search)
  const state = p.get('state')
  if (!state) return null
  const code = p.get('code')
  if (code) return { code, state }
  const error = p.get('error')
  return error ? { error: p.get('error_description') ?? error } : null
}

/* ------------------------------------------------------------ pieces --- */

export function Field({
  label,
  type = 'text',
  value,
  onChange,
  autoComplete,
  hint,
  inputMode,
}: {
  label: string
  type?: string
  value: string
  onChange: (v: string) => void
  autoComplete?: string
  hint?: string
  inputMode?: 'numeric' | 'email' | 'text'
}) {
  return (
    <label className="mt-4 block first:mt-0">
      <span className="legend mb-1.5 block text-ink-3">{label}</span>
      <input
        type={type}
        autoComplete={autoComplete}
        inputMode={inputMode}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="well w-full px-3 py-2 text-[0.9375rem] text-ink outline-none"
      />
      {hint && <span className="mt-1.5 block text-[0.8125rem] text-ink-3">{hint}</span>}
    </label>
  )
}

export function Submit({ busy, disabled, children }: { busy: boolean; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="submit"
      disabled={busy || disabled}
      className="legend panel-raised mt-5 w-full px-3 py-2.5 text-ink transition-colors hover:bg-panel-3 disabled:opacity-60"
    >
      {busy ? 'Working' : children}
    </button>
  )
}

export function Alert({ text, tone = 'error' }: { text: string | null; tone?: 'error' | 'note' }) {
  if (!text) return null
  return (
    <p
      role={tone === 'error' ? 'alert' : 'status'}
      className={`mt-4 text-[0.875rem] leading-relaxed ${tone === 'error' ? 'text-oxide' : 'text-ink-2'}`}
    >
      {text}
    </p>
  )
}

function Frame({ title, lede, children, endpoint }: { title: string; lede: string; children: ReactNode; endpoint: string }) {
  return (
    <div className="mx-auto max-w-[28rem] px-5 py-14 sm:py-20">
      <h2 className="display text-[clamp(1.5rem,3vw,2rem)]">{title}</h2>
      <p className="mt-3 text-[0.9375rem] leading-relaxed text-ink-2">{lede}</p>
      {children}
      <p className="data mt-8 text-[0.75rem] text-ink-3">{endpoint.replace(/^https?:\/\//, '')}</p>
    </div>
  )
}

/* ------------------------------------------------------- the screen --- */

type Mode = 'login' | 'signup' | 'forgot' | 'sso'

export function AuthScreen({
  endpoint,
  signup,
  sso,
  onSession,
}: {
  endpoint: string
  /** The daemon offers public sign-up. */
  signup: boolean
  /** Some organisation on the daemon logs in with single sign-on. */
  sso: boolean
  onSession: (s: AccountSession) => void
}) {
  const o = { endpoint }
  const [link] = useState(linkFromUrl)
  const [callback] = useState(ssoCallbackFromUrl)
  const [mode, setMode] = useState<Mode>(link?.kind === 'forgot' ? 'forgot' : signup ? 'signup' : 'login')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [needCode, setNeedCode] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const done = (s: AccountSession) => {
    saveSession(endpoint, s)
    clearLink()
    onSession(s)
  }

  // A single sign-on callback finishes on its own.
  useEffect(() => {
    if (!callback) return
    if ('error' in callback) {
      setError(`The identity provider refused: ${callback.error}`)
      clearLink()
      return
    }
    const binding = sessionStorage.getItem(SSO_BINDING) ?? ''
    sessionStorage.removeItem(SSO_BINDING)
    setBusy(true)
    accountCall<AccountSession>('POST', '/v1/api/sso/callback', o, { ...callback, binding })
      .then(done)
      .catch((e) => {
        clearLink()
        setError(failureText(e))
      })
      .finally(() => setBusy(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  if (link?.kind === 'verify' && link.token) return <VerifyLink endpoint={endpoint} token={link.token} onSession={done} />
  if (link?.kind === 'reset' && link.token) return <ResetLink endpoint={endpoint} token={link.token} onSession={done} />
  if (link?.kind === 'email' && link.token) return <EmailLink endpoint={endpoint} token={link.token} />
  if (link?.kind === 'invite' && link.token) return <InviteLink endpoint={endpoint} token={link.token} onSession={done} />

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setError(null)
    setNotice(null)
    setBusy(true)
    try {
      if (mode === 'signup') {
        if ([...password].length < MIN_PASSWORD) {
          setError(`The password needs at least ${MIN_PASSWORD} characters.`)
          return
        }
        const n = await api.signup(email, password, o)
        setNotice(n.message)
        setPassword('')
      } else if (mode === 'login') {
        try {
          done(await api.login(email, password, needCode ? code : undefined, o))
        } catch (err) {
          if (errorCode(err) === 'totp_required') {
            setNeedCode(true)
            if (needCode) setError(failureText(err))
            return
          }
          if (errorCode(err) === 'sso_required') {
            setMode('sso')
          }
          throw err
        }
      } else if (mode === 'forgot') {
        const n = await accountCall<AccountNotice>('POST', '/v1/api/account/password/forgot', o, { email })
        setNotice(n.message)
      } else {
        const start = await accountCall<SsoStart>('POST', '/v1/api/sso/start', o, { email })
        sessionStorage.setItem(SSO_BINDING, start.binding)
        location.assign(start.authorize_url)
      }
    } catch (err) {
      setError(failureText(err))
    } finally {
      setBusy(false)
    }
  }

  const titles: Record<Mode, [string, string]> = {
    login: ['Log in', 'Log in to read your projects and manage their keys.'],
    signup: ['Create an account', 'You get a project of your own and the keys to write to it. Every feature is open.'],
    forgot: ['Reset your password', 'We send a link to the address, if an account uses it.'],
    sso: ['Log in with SSO', 'Your organisation signs you in. Give your work address and we send you to its sign-in page.'],
  }
  const [title, lede] = titles[mode]
  const switchTo = (m: Mode) => {
    setMode(m)
    setError(null)
    setNotice(null)
    setNeedCode(false)
  }

  return (
    <Frame title={title} lede={lede} endpoint={endpoint}>
      <form onSubmit={submit} className="panel mt-7 p-5" noValidate>
        <Field label="Email" type="email" autoComplete="email" value={email} onChange={setEmail} inputMode="email" />
        {(mode === 'login' || mode === 'signup') && (
          <Field
            label="Password"
            type="password"
            autoComplete={mode === 'signup' ? 'new-password' : 'current-password'}
            value={password}
            onChange={setPassword}
            hint={mode === 'signup' ? `At least ${MIN_PASSWORD} characters.` : undefined}
          />
        )}
        {mode === 'login' && needCode && (
          <Field
            label="Code from your authenticator app"
            autoComplete="one-time-code"
            inputMode="numeric"
            value={code}
            onChange={setCode}
            hint="Or one of your recovery codes."
          />
        )}
        <Alert text={error} />
        <Alert text={notice} tone="note" />
        <Submit busy={busy} disabled={!email || ((mode === 'login' || mode === 'signup') && !password)}>
          {mode === 'login' ? 'Log in' : mode === 'signup' ? 'Create account' : mode === 'forgot' ? 'Send link' : 'Continue'}
        </Submit>
      </form>

      <div className="mt-5 flex flex-col gap-2 text-[0.875rem] text-ink-2">
        {mode !== 'login' && (
          <p>
            Have an account?{' '}
            <button type="button" onClick={() => switchTo('login')} className="link">
              Log in
            </button>
          </p>
        )}
        {mode !== 'signup' && signup && (
          <p>
            No account yet?{' '}
            <button type="button" onClick={() => switchTo('signup')} className="link">
              Create one
            </button>
          </p>
        )}
        {mode === 'login' && (
          <p>
            <button type="button" onClick={() => switchTo('forgot')} className="link">
              Forgot your password?
            </button>
          </p>
        )}
        {mode !== 'sso' && sso && (
          <p>
            <button type="button" onClick={() => switchTo('sso')} className="link">
              Log in with SSO
            </button>
          </p>
        )}
      </div>
    </Frame>
  )
}

/* -------------------------------------------------------- the links --- */

function VerifyLink({ endpoint, token, onSession }: { endpoint: string; token: string; onSession: (s: AccountSession) => void }) {
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    accountCall<AccountSession>('POST', '/v1/api/account/verify', { endpoint }, { token })
      .then(onSession)
      .catch((e) => {
        clearLink()
        setError(failureText(e))
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <Frame title="Confirming your address" lede="One moment." endpoint={endpoint}>
      <Alert text={error} />
      {error && (
        <p className="mt-5 text-[0.875rem]">
          <a href={location.pathname} className="link">
            Back to log in
          </a>
        </p>
      )}
    </Frame>
  )
}

function ResetLink({ endpoint, token, onSession }: { endpoint: string; token: string; onSession: (s: AccountSession) => void }) {
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [needCode, setNeedCode] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setError(null)
    if ([...password].length < MIN_PASSWORD) {
      setError(`The password needs at least ${MIN_PASSWORD} characters.`)
      return
    }
    setBusy(true)
    try {
      onSession(
        await accountCall<AccountSession>('POST', '/v1/api/account/password/reset', { endpoint }, {
          token,
          password,
          totp_code: needCode ? code : undefined,
        }),
      )
    } catch (err) {
      if (errorCode(err) === 'totp_required' && !needCode) setNeedCode(true)
      else setError(failureText(err))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Frame title="Choose a new password" lede="Every other session of the account ends." endpoint={endpoint}>
      <form onSubmit={submit} className="panel mt-7 p-5" noValidate>
        <Field label="New password" type="password" autoComplete="new-password" value={password} onChange={setPassword} hint={`At least ${MIN_PASSWORD} characters.`} />
        {needCode && <Field label="Code from your authenticator app" inputMode="numeric" autoComplete="one-time-code" value={code} onChange={setCode} />}
        <Alert text={error} />
        <Submit busy={busy} disabled={!password}>Set password</Submit>
      </form>
    </Frame>
  )
}

function EmailLink({ endpoint, token }: { endpoint: string; token: string }) {
  const [state, setState] = useState<{ ok?: string; error?: string }>({})
  useEffect(() => {
    accountCall<Account>('POST', '/v1/api/account/email/confirm', { endpoint }, { token })
      .then((a) => setState({ ok: `Your address is now ${a.email}.` }))
      .catch((e) => setState({ error: failureText(e) }))
      .finally(clearLink)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <Frame title="Confirming your new address" lede={state.ok ?? 'One moment.'} endpoint={endpoint}>
      <Alert text={state.error ?? null} />
      {(state.ok || state.error) && (
        <p className="mt-5 text-[0.875rem]">
          <a href={location.pathname} className="link">
            Continue
          </a>
        </p>
      )}
    </Frame>
  )
}

/**
 * An invitation. With no account for the address, a password creates one. With
 * one, the reader logs in as it and opens the link again; a logged-in reader
 * accepts from the account screen.
 */
export function InviteLink({
  endpoint,
  token,
  onSession,
  loggedIn = false,
  onAccepted,
}: {
  endpoint: string
  token: string
  onSession?: (s: AccountSession) => void
  loggedIn?: boolean
  onAccepted?: () => void
}) {
  const [info, setInfo] = useState<InviteLookup | null>(null)
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    accountCall<InviteLookup>('POST', '/v1/api/invites/lookup', { endpoint }, { token })
      .then(setInfo)
      .catch((e) => setError(failureText(e)))
  }, [endpoint, token])
  const accept = async (e: FormEvent) => {
    e.preventDefault()
    setError(null)
    setBusy(true)
    try {
      const r = await accountCall<InviteAccepted>('POST', '/v1/api/invites/accept', { endpoint }, {
        token,
        password: info?.account_exists ? undefined : password,
      })
      clearLink()
      if (r.session && onSession) onSession(r.session)
      else onAccepted?.()
    } catch (err) {
      setError(failureText(err))
    } finally {
      setBusy(false)
    }
  }
  const body = (
    <form onSubmit={accept} className="panel mt-7 p-5" noValidate>
      {info && (
        <p className="text-[0.875rem] leading-relaxed text-ink">
          Join <strong>{info.org_name}</strong> as <span className="data">{info.role}</span>, as {info.email}.
        </p>
      )}
      {info && !info.account_exists && (
        <div className="mt-4">
          <Field label="Choose a password" type="password" autoComplete="new-password" value={password} onChange={setPassword} hint={`At least ${MIN_PASSWORD} characters.`} />
        </div>
      )}
      {info?.account_exists && !loggedIn && (
        <p className="mt-3 text-[0.875rem] text-ink-2">An account uses this address. Log in as it, then open the link again.</p>
      )}
      <Alert text={error} />
      {info && (info.account_exists ? loggedIn : true) && (
        <Submit busy={busy} disabled={!info.account_exists && !password}>Join</Submit>
      )}
    </form>
  )
  if (loggedIn) return body
  return (
    <Frame title="An invitation" lede="Somebody invited you to an organisation on mcpd." endpoint={endpoint}>
      {body}
      {info?.account_exists && (
        <p className="mt-5 text-[0.875rem]">
          <a href={location.pathname} className="link" onClick={() => sessionStorage.setItem(PENDING_INVITE, token)}>
            Log in
          </a>
        </p>
      )}
    </Frame>
  )
}
