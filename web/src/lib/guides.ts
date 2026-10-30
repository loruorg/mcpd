/**
 * The task guides: one page per job, at `/docs/guides/<slug>`.
 *
 * One list, read by the index, the reading order between guides and the
 * structured data, so the three cannot disagree about which guides exist. The
 * guides are written in English only. The docs sidebar carries one entry for
 * the whole set (`/docs/guides` in `docs.ts`), so a translated sidebar gains
 * one link to the English index rather than eleven links to pages that do not
 * exist in its language.
 */
import { SITE } from '@/lib/site'

export interface Guide {
  slug: string
  /** The page's H1: the task, as a reader would search for it. */
  title: string
  /** The `<title>`, when the H1 is too long for one. The layout adds " · mcpd". */
  short?: string
  /** The meta description. Under 155 characters. */
  description: string
  /** One line for the index. */
  blurb: string
}

export const GUIDES_PATH = '/docs/guides'

/** The day the guides' content last changed, shown on each page and in its JSON-LD. */
export const GUIDES_UPDATED = '2026-10-30'

/** In reading order: instrument first, then read, then act on what you read. */
export const GUIDES: Guide[] = [
  {
    slug: 'log-openai-api-calls',
    title: 'How to log OpenAI API calls with tokens and cost',
    description:
      'Record every OpenAI chat completion with its prompt, answer, token counts, cached tokens and cost, in TypeScript or Python, streamed or not.',
    blurb: 'Every chat completion with its prompt, answer, tokens and cost, streamed or not.',
  },
  {
    slug: 'trace-langchain-agents',
    title: 'How to trace LangChain and LangGraph agents',
    description:
      'Send LangChain and LangGraph agent traces to mcpd with the OpenInference instrumentor and an OTLP exporter: model calls, tools, tokens and cost.',
    blurb: 'OpenInference and one OTLP exporter: every chain, model call and tool in the waterfall.',
  },
  {
    slug: 'trace-vercel-ai-sdk',
    title: 'How to trace the Vercel AI SDK',
    description:
      'Trace generateText and streamText from the Vercel AI SDK into mcpd over OpenTelemetry, with prompts, tool calls, tokens and correct cost totals.',
    blurb: 'AI SDK telemetry over OTLP, with prompts, tool calls and cost totals that add up.',
  },
  {
    slug: 'trace-pydantic-ai',
    title: 'How to trace Pydantic AI agents with OpenTelemetry',
    description:
      'Send Pydantic AI agent runs to mcpd with plain OpenTelemetry, no Logfire account: model requests, tool calls, prompts, tokens and cost.',
    blurb: 'Agent.instrument_all() and an OTLP exporter, with prompts you can search.',
  },
  {
    slug: 'trace-chat-conversations',
    title: 'How to group LLM traces into conversations',
    description:
      'Give every turn of a chat the same session id, then list, measure and read a whole conversation in mcpd, from the SDKs or OpenTelemetry.',
    blurb: 'One trace per turn, one session id per conversation, and the queries that read it back.',
  },
  {
    slug: 'llm-cost-per-user',
    title: 'How to track LLM cost per user',
    description:
      'Tag each run with the end user, then get cost and tokens per user from the mcpd API, with user ids hashed at ingest and mapped back on your side.',
    blurb: 'Tag runs with the end user, then break spend down by user without storing who they are.',
  },
  {
    slug: 'collect-user-feedback',
    title: 'How to collect thumbs-up and thumbs-down feedback on LLM answers',
    short: 'How to collect user feedback on LLM answers',
    description:
      'Add thumbs up and down buttons to an LLM feature, store each vote on the trace that produced the answer, and list the runs people disliked.',
    blurb: 'A vote from your product lands on the exact run that produced the answer.',
  },
  {
    slug: 'slack-alerts',
    title: 'How to send LLM alerts to Slack',
    description:
      'Watch error rate, latency or spend with an mcpd monitor and post each alert to a Slack channel through a small webhook relay.',
    blurb: 'A monitor on error rate or spend, and a 30-line relay that posts it to a channel.',
  },
  {
    slug: 'llm-regression-tests-github-actions',
    title: 'How to run LLM regression tests in GitHub Actions',
    description:
      'Run your agent over a fixed dataset on every pull request, score the answers in mcpd, and fail the job when a score regresses against main.',
    blurb: 'Score a dataset on every pull request and fail the job on a real regression.',
  },
  {
    slug: 'human-approval-step',
    title: 'How to add a human approval step to an AI agent',
    description:
      'Pause an agent before it acts, wait days for a person to approve or reject, and resume where it stopped, with no process kept alive while it waits.',
    blurb: 'A durable wait that holds the run until a person says yes or no.',
  },
  {
    slug: 'read-traces-in-claude-code',
    title: 'How to read agent traces in Claude Code and Cursor',
    description:
      'Connect Claude Code or Cursor to mcpd over MCP so your coding agent can find a failed run, read its steps and prompts, and suggest the fix.',
    blurb: 'Give a coding agent a read key and three tools: find a run, open it, compare it.',
  },
]

export const guidePath = (slug: string) => `${GUIDES_PATH}/${slug}`

export function guide(slug: string): Guide {
  const g = GUIDES.find((x) => x.slug === slug)
  // A guide page that names a slug the list does not hold would publish with
  // no title and no reading order. Failing the build names the typo instead.
  if (!g) throw new Error(`no guide "${slug}" in web/src/lib/guides.ts`)
  return g
}

/** Previous and next guide, in the order of `GUIDES`. The first one leads back to the index. */
export function guideNeighbours(slug: string) {
  const i = GUIDES.findIndex((g) => g.slug === slug)
  const entry = (g: Guide | undefined) => (g ? { href: guidePath(g.slug), title: g.title } : undefined)
  return {
    prev: i === 0 ? { href: GUIDES_PATH, title: 'Guides' } : entry(GUIDES[i - 1]),
    next: entry(GUIDES[i + 1]),
  }
}

const absolute = (p: string) => new URL(p, SITE.url).href

/**
 * JSON for a `<script type="application/ld+json">` element, with `<` escaped so
 * a title can never end the element early.
 */
const jsonLd = (value: unknown) => JSON.stringify(value).replace(/</g, '\\u003c')

const crumbs = (tail: { name: string; path: string }[]) => ({
  '@context': 'https://schema.org',
  '@type': 'BreadcrumbList',
  itemListElement: [
    { name: SITE.name, path: '/' },
    { name: 'Docs', path: '/docs' },
    { name: 'Guides', path: GUIDES_PATH },
    ...tail,
  ].map((c, i) => ({
    '@type': 'ListItem',
    position: i + 1,
    name: c.name,
    item: c.path === '/' ? SITE.url : absolute(c.path),
  })),
})

/** TechArticle and BreadcrumbList for one guide. */
export function guideSchema(g: Guide): string[] {
  const url = absolute(guidePath(g.slug))
  return [
    jsonLd({
      '@context': 'https://schema.org',
      '@type': 'TechArticle',
      headline: g.title,
      description: g.description,
      url,
      mainEntityOfPage: url,
      inLanguage: 'en',
      dateModified: GUIDES_UPDATED,
      isPartOf: { '@type': 'WebSite', name: SITE.name, url: SITE.url },
      about: { '@type': 'SoftwareApplication', name: SITE.name, url: SITE.url },
    }),
    jsonLd(crumbs([{ name: g.title, path: guidePath(g.slug) }])),
  ]
}

/** CollectionPage with an ItemList of every guide, and the trail to the index. */
export function guidesIndexSchema(title: string, description: string): string[] {
  const url = absolute(GUIDES_PATH)
  return [
    jsonLd({
      '@context': 'https://schema.org',
      '@type': 'CollectionPage',
      name: title,
      description,
      url,
      inLanguage: 'en',
      dateModified: GUIDES_UPDATED,
      isPartOf: { '@type': 'WebSite', name: SITE.name, url: SITE.url },
      mainEntity: {
        '@type': 'ItemList',
        numberOfItems: GUIDES.length,
        itemListElement: GUIDES.map((g, i) => ({
          '@type': 'ListItem',
          position: i + 1,
          name: g.title,
          url: absolute(guidePath(g.slug)),
        })),
      },
    }),
    jsonLd({
      '@context': 'https://schema.org',
      '@type': 'BreadcrumbList',
      itemListElement: [
        { '@type': 'ListItem', position: 1, name: SITE.name, item: SITE.url },
        { '@type': 'ListItem', position: 2, name: 'Docs', item: absolute('/docs') },
        { '@type': 'ListItem', position: 3, name: 'Guides', item: url },
      ],
    }),
  ]
}
