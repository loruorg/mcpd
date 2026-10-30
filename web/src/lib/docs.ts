/**
 * The documentation map.
 *
 * One list, used for the sidebar, the previous/next links, the search index and
 * the sitemap. Adding a page here and forgetting to create it fails the build,
 * which is the point: a navigation entry that 404s is worse than a missing page.
 */

export interface DocEntry {
  href: string
  title: string
  /** One line, shown under the title in the index and used as the meta description fallback. */
  blurb: string
}

export interface DocSection {
  legend: string
  entries: DocEntry[]
}

export const DOCS: DocSection[] = [
  {
    legend: 'Start',
    entries: [
      { href: '/docs', title: 'Overview', blurb: 'The daemon between your agent\'s steps, and the record of each one.' },
      { href: '/docs/quickstart', title: 'Quickstart', blurb: 'Daemon running and a trace on screen, in five minutes.' },
      { href: '/docs/install', title: 'Install', blurb: 'Docker, Compose, the Helm chart, Cargo, or built from source.' },
      { href: '/docs/concepts', title: 'Concepts', blurb: 'Traces, spans, the six signals, and the words mcpd uses for them.' },
      { href: '/docs/from-mastra', title: 'Coming from Mastra', blurb: 'Where mcpd is ahead, where it is behind, and how to move.' },
      { href: '/docs/guides', title: 'Guides', blurb: 'One job per page: trace a framework, cost per user, alerts, CI and approvals.' },
    ],
  },
  {
    legend: 'Run',
    entries: [
      { href: '/docs/workflows', title: 'Workflows', blurb: 'The definition the daemon walks: nine constructs, templates, guards, retries and effects.' },
      { href: '/docs/workers', title: 'Workers', blurb: 'Your process serving the steps: leases, heartbeats, the four refusals, scaling out.' },
      { href: '/docs/runs', title: 'Runs', blurb: 'Starting, following, answering, cancelling, and where a run appears in its trace.' },
    ],
  },
  {
    legend: 'Instrument',
    entries: [
      { href: '/docs/integrating', title: 'Integrating', blurb: 'The HTTP API on its own, the six SDKs, or mcpd as an MCP server.' },
      { href: '/docs/opentelemetry', title: 'OpenTelemetry', blurb: 'Point an existing OTel setup at mcpd and send no new code.' },
      { href: '/docs/sdks/typescript', title: 'TypeScript', blurb: 'Node, Deno, Bun and the edge.' },
      { href: '/docs/sdks/python', title: 'Python', blurb: 'Sync and async, no required dependencies.' },
      { href: '/docs/sdks/rust', title: 'Rust', blurb: 'An RAII span guard and a task-local context.' },
      { href: '/docs/sdks/go', title: 'Go', blurb: 'context.Context throughout, standard library only.' },
      { href: '/docs/sdks/kotlin', title: 'Kotlin', blurb: 'Coroutine context and structured concurrency.' },
      { href: '/docs/sdks/elixir', title: 'Elixir', blurb: 'A GenServer worker, the process for context, and no dependencies.' },
    ],
  },
  {
    legend: 'Operate',
    entries: [
      { href: '/docs/searching', title: 'Finding a run', blurb: 'Full-text search, filters, and the query that finds bad answers.' },
      { href: '/docs/cost', title: 'Cost accounting', blurb: 'How a figure is produced, and how to price your own rates.' },
      { href: '/docs/evaluation', title: 'Scores and feedback', blurb: 'Machine evaluation and human judgement, kept apart on purpose.' },
      { href: '/docs/aspects', title: 'Aspect notes', blurb: 'One sentence about a finished run, written by your model and not by mcpd.' },
      { href: '/docs/themes', title: 'Themes and snapshots', blurb: 'Notes grouped by shared wording, with a floor before anything is reported.' },
      { href: '/docs/prompts', title: 'Prompts', blurb: 'Versioned prompts with labels, fetched by the SDKs and linked to the steps that ran them.' },
      { href: '/docs/testing', title: 'Regression testing', blurb: 'Capture a failed run as a case, score it, and compare two versions.' },
      { href: '/docs/alerting', title: 'Monitors and alerts', blurb: 'A condition watched on a timer, latched, and reported once.' },
      { href: '/docs/privacy', title: 'Redaction and retention', blurb: 'What never gets stored, what stops being stored, and how to erase one person.' },
      { href: '/docs/sampling', title: 'Sampling', blurb: 'Keeping a fraction of traces without keeping a fraction of each one.' },
    ],
  },
  {
    legend: 'Deploy',
    entries: [
      { href: '/deploy', title: 'Docker and Kubernetes', blurb: 'Compose file, manifests, Helm chart, and what to set before exposing it.' },
      { href: '/docs/configuration', title: 'Configuration', blurb: 'Every flag and environment variable.' },
      { href: '/docs/accounts', title: 'Accounts and organisations', blurb: 'Roles, single sign-on, SCIM, two-step log-in and quotas.' },
      { href: '/docs/operations', title: 'Running it', blurb: 'Probes, metrics, backups, upgrades and scale.' },
      { href: '/docs/forwarding', title: 'Forwarding', blurb: 'Copy what mcpd stores on to Langfuse, Datadog, any OTLP receiver, or several at once.' },
      { href: '/docs/relay', title: 'Relay and regions', blurb: 'An edge collector that spools and fails over, and several regions on one store.' },
      { href: '/docs/security', title: 'Security', blurb: 'What is stored, what leaves the machine, and what mcpd does not do.' },
    ],
  },
  {
    legend: 'Reference',
    entries: [
      { href: '/docs/reference/data-model', title: 'Data model', blurb: 'Every entity and field, generated from the specification.' },
      { href: '/docs/reference/api', title: 'HTTP API', blurb: 'Every endpoint, generated from the specification.' },
    ],
  },
]

export const ALL_DOCS: DocEntry[] = DOCS.flatMap((s) => s.entries)

/** Previous and next in reading order, for the footer of a page. */
export function neighbours(href: string) {
  const i = ALL_DOCS.findIndex((d) => d.href === href)
  return {
    prev: i > 0 ? ALL_DOCS[i - 1] : undefined,
    next: i >= 0 && i < ALL_DOCS.length - 1 ? ALL_DOCS[i + 1] : undefined,
  }
}
