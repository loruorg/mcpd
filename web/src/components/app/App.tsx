/**
 * The operator interface, and the way between its screens.
 *
 * Every screen reads the same daemon: the runs explorer, one run read back as
 * the conversation its steps encode, the threads those runs group into, the
 * log stream, the spend breakdown and the durable runs, then evaluations,
 * datasets, themes, metrics and monitors (`more-screens.tsx`). They are one
 * application rather than six pages because the whole interface is a static
 * bundle that the daemon serves itself, and because moving from a log line to
 * the run that wrote it should not cost a page load.
 *
 * The screen lives in the URL, so a link into a run survives a reload and the
 * browser's back button does what it looks like it does.
 */

import { useCallback, useEffect, useState } from 'react'
import { findDaemon, storedSession } from '@/lib/api'
import type { StoredSession } from '@/lib/api'
import Explorer, { initialEndpoint, initialProject } from './Explorer'
import { AccountPanel, AuthScreen } from './Account'
import { linkFromUrl } from './AccountAuth'
import Logs from './Logs'
import Cost from './Cost'
import Workflows from './Workflows'
import Messages from './Messages'
import Threads from './ThreadPanel'
import { MORE_SCREENS } from './more-screens'
import type { MoreScreenId } from './more-screens'

/* `runs` is the trace explorer, the first screen and the name every link into
   it has carried; the durable runs the daemon walks are `workflows`. */
type ScreenId =
  | 'runs'
  | 'messages'
  | 'threads'
  | 'logs'
  | 'spend'
  | 'workflows'
  | 'account'
  | MoreScreenId

const SCREENS: { id: ScreenId; label: string }[] = [
  { id: 'runs', label: 'Runs' },
  { id: 'messages', label: 'Conversation' },
  { id: 'threads', label: 'Threads' },
  { id: 'logs', label: 'Logs' },
  { id: 'spend', label: 'Spend' },
  { id: 'workflows', label: 'Workflows' },
  ...MORE_SCREENS.map(({ id, label }) => ({ id, label })),
]

/* Only on a daemon that offers accounts, and only once logged in. */
const ACCOUNT_SCREEN: { id: ScreenId; label: string } = { id: 'account', label: 'Account' }

function isScreen(v: string | null): v is ScreenId {
  return v === 'account' || SCREENS.some((s) => s.id === v)
}

interface Route {
  screen: ScreenId
  trace?: string
  run?: string
  thread?: string
}

function fromUrl(): Route {
  if (typeof window === 'undefined') return { screen: 'runs' }
  const p = new URLSearchParams(location.search)
  const s = p.get('screen')
  return {
    screen: isScreen(s) ? s : 'runs',
    trace: p.get('trace') ?? undefined,
    run: p.get('run') ?? undefined,
    thread: p.get('thread') ?? undefined,
  }
}

export default function App() {
  const [{ screen: routed, trace, run, thread }, setRoute] = useState(fromUrl)
  const [endpoint, setEndpoint] = useState(initialEndpoint)
  // Held here rather than inside each screen, so choosing a project on the log
  // stream still applies when you move to spend.
  const [project, setProject] = useState(initialProject)
  // Whether this daemon offers accounts, from its health probe, and the
  // session this browser holds for it. `null` until the probe answers, so a
  // daemon without accounts never flashes a log-in screen.
  // `signup` here means "accounts are on", with or without public sign-up;
  // `offers` says which doors the log-in screen shows.
  const [signup, setSignup] = useState<boolean | null>(null)
  const [offers, setOffers] = useState({ signup: false, sso: false })
  const [session, setSession] = useState<StoredSession | null>(() => storedSession(endpoint))
  const [mailedLink, setMailedLink] = useState(linkFromUrl)

  useEffect(() => {
    let live = true
    setSession(storedSession(endpoint))
    findDaemon(endpoint)
      .then(({ health }) => {
        if (!live) return
        setSignup(health.signup === true || health.accounts === true)
        setOffers({ signup: health.signup === true, sso: health.sso === true })
      })
      .catch(() => live && setSignup(false))
    return () => {
      live = false
    }
  }, [endpoint])

  /* One way between screens, so every link carries the same subjects. A
     parameter left out is cleared rather than inherited: arriving at the log
     stream with the previous screen's trace still on the URL would filter it to
     a run nobody asked about. */
  const go = useCallback((next: ScreenId, where: { trace?: string; thread?: string } = {}) => {
    setRoute({ screen: next, trace: where.trace, thread: where.thread })
    const p = new URLSearchParams(location.search)
    p.set('screen', next)
    if (where.trace) p.set('trace', where.trace)
    else p.delete('trace')
    if (where.thread) p.set('thread', where.thread)
    else p.delete('thread')
    p.delete('run')
    history.pushState({ screen: next, ...where }, '', `${location.pathname}?${p}`)
  }, [])

  useEffect(() => {
    const onPop = () => setRoute(fromUrl())
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])

  // A mailed link other than an invitation is about an account, possibly not
  // the one logged in here, so it opens the account screens to be finished.
  if (signup && (!session || (mailedLink && mailedLink.kind !== 'invite'))) {
    return (
      <AuthScreen
        endpoint={endpoint}
        signup={offers.signup}
        sso={offers.sso}
        onSession={(s) => {
          setMailedLink(null)
          setSession(storedSession(endpoint))
          if (s.project) setProject(s.project)
        }}
      />
    )
  }

  const screens = signup && session ? [...SCREENS, ACCOUNT_SCREEN] : SCREENS
  // A link to the account screen on a daemon without accounts opens the runs.
  const screen: ScreenId = routed === 'account' && !(signup && session) ? 'runs' : routed

  return (
    <div>
      {/* The function selector. Not a tab strip and not a rail of icons: the
          current screen is marked in the type itself, by weight and by ink,
          which is the only mark on this panel that does not also mean
          something else. */}
      <nav aria-label="Operator screens" className="border-b border-scribe bg-panel-2">
        <div className="flex flex-wrap items-baseline gap-x-7 gap-y-2 px-4 py-3">
          {screens.map((s) => {
            const on = s.id === screen
            return (
              <button
                key={s.id}
                type="button"
                onClick={() => go(s.id)}
                aria-current={on ? 'page' : undefined}
                className={`display text-[0.9375rem] transition-colors ${
                  on ? 'text-ink' : 'font-medium text-ink-3 hover:text-ink'
                }`}
              >
                {s.label}
              </button>
            )
          })}
          <span
            className="data ml-auto max-w-[16rem] truncate text-[0.6875rem] text-ink-3"
            title={`This interface is reading ${endpoint}`}
          >
            {endpoint.replace(/^https?:\/\//, '')}
          </span>
        </div>
      </nav>

      {screen === 'runs' && (
        // Keyed on the run so arriving from a log line opens that run rather
        // than the newest one, even when the explorer is already mounted.
        <Explorer
          key={trace ?? 'newest'}
          initialTrace={trace}
          endpoint={endpoint}
          onEndpoint={setEndpoint}
        />
      )}
      {screen === 'messages' && (
        <Messages
          key={trace ?? 'newest'}
          initialTrace={trace}
          endpoint={endpoint}
          onEndpoint={setEndpoint}
          project={project}
          onProject={setProject}
          onOpenThread={(threadId, traceId) => go('threads', { thread: threadId, trace: traceId })}
        />
      )}
      {screen === 'threads' && (
        <Threads
          key={`${thread ?? 'none'}:${trace ?? 'none'}`}
          initialThread={thread}
          initialTrace={trace}
          endpoint={endpoint}
          onEndpoint={setEndpoint}
          project={project}
          onProject={setProject}
          onOpenConversation={(traceId) => go('messages', { trace: traceId })}
        />
      )}
      {screen === 'logs' && (
        <Logs
          endpoint={endpoint}
          onEndpoint={setEndpoint}
          project={project}
          onProject={setProject}
          onOpenTrace={(id) => go('runs', { trace: id })}
        />
      )}
      {screen === 'workflows' && (
        <Workflows
          endpoint={endpoint}
          onEndpoint={setEndpoint}
          project={project}
          onProject={setProject}
          onOpenTrace={(id) => go('runs', { trace: id })}
          initialRun={run}
        />
      )}
      {screen === 'account' && session && (
        <AccountPanel
          endpoint={endpoint}
          session={session}
          onLogout={() => {
            setSession(null)
            go('runs')
          }}
        />
      )}
      {MORE_SCREENS.map(
        ({ id, Screen }) =>
          screen === id && (
            <Screen
              key={id}
              endpoint={endpoint}
              onEndpoint={setEndpoint}
              project={project}
              onProject={setProject}
              onOpenTrace={(t) => go('runs', { trace: t })}
            />
          ),
      )}
      {screen === 'spend' && (
        <Cost
          endpoint={endpoint}
          onEndpoint={setEndpoint}
          project={project}
          onProject={setProject}
        />
      )}
    </div>
  )
}
