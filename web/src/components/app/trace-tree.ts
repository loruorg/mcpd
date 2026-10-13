/**
 * The model behind the trace panel: the tree, what is open, what a search kept
 * and why.
 *
 * All of it is pure. The panel renders what these functions return, which is
 * what makes the waterfall and the inspector agree: prev/next walks the same
 * array the rows are drawn from, so the two can never disagree about what the
 * next span is.
 */

import type { Span, SpanKind } from '@/generated/types'

/* ------------------------------------------------------------- colour --- */

/**
 * The colour a kind is drawn in, by family.
 *
 * Five colours against thirty-three kinds, so the mapping is by family and the
 * legend names the kinds actually present rather than the families. The signal
 * colours keep the meaning they carry everywhere else on this panel: amber is
 * something reaching outside the process, oxide is a gate that can refuse,
 * olive is a measurement of the run itself.
 *
 * `text` and `bar` are separate because ink-4 is a rule colour in this system
 * and never carries text: a quiet kind gets a quiet bar and readable ink.
 */
export interface KindColour {
  text: string
  bar: string
}

const RUN: KindColour = { text: 'text-ink', bar: 'var(--color-ink)' }
const MODEL: KindColour = { text: 'text-slate', bar: 'var(--color-slate)' }
const TOOL: KindColour = { text: 'text-amber', bar: 'var(--color-amber)' }
const EVAL: KindColour = { text: 'text-olive', bar: 'var(--color-olive)' }
const GATE: KindColour = { text: 'text-oxide', bar: 'var(--color-oxide)' }
const QUIET: KindColour = { text: 'text-ink-3', bar: 'var(--color-ink-4)' }

const KIND_COLOUR: Partial<Record<SpanKind, KindColour>> = {
  agent_run: RUN,
  workflow_run: RUN,
  workflow_step: RUN,
  workflow_conditional: RUN,
  workflow_conditional_eval: RUN,
  workflow_parallel: RUN,
  workflow_loop: RUN,
  workflow_sleep: RUN,
  workflow_wait_event: RUN,
  processor_run: RUN,
  handoff: RUN,

  model_generation: MODEL,
  model_step: MODEL,
  model_inference: MODEL,
  model_chunk: MODEL,
  rag_embedding: MODEL,

  tool_call: TOOL,
  client_tool_call: TOOL,
  provider_tool_call: TOOL,
  mcp_tool_call: TOOL,
  http_request: TOOL,
  workspace_action: TOOL,
  graph_action: TOOL,

  scorer_run: EVAL,
  scorer_step: EVAL,

  guardrail: GATE,
}

export function kindColour(kind: string): KindColour {
  return KIND_COLOUR[kind as SpanKind] ?? QUIET
}

/** A kind as a reader says it. */
export function kindLabel(kind: string): string {
  return kind.replace(/_/g, ' ')
}

/* --------------------------------------------------------------- tree --- */

export interface TreeNode {
  span: Span
  depth: number
  children: TreeNode[]
}

/** One line of the waterfall, in the order it is drawn. */
export interface Row {
  span: Span
  depth: number
  hasChildren: boolean
  /** How many spans hang below this one, at any depth. */
  descendants: number
  parentId?: string
}

/**
 * The parent chain, as a tree.
 *
 * A span whose parent has not arrived becomes a root rather than disappearing.
 * That case is not hypothetical: spans are written as they end, so a child can
 * be stored before its parent, and hiding it would mean the panel showed fewer
 * steps than the run's own span count.
 *
 * Children keep the order the daemon sent them in, which is start time then
 * depth, so the waterfall does not re-sort a run it was handed in order.
 */
export function buildTree(spans: Span[]): TreeNode[] {
  const byId = new Map(spans.map((s) => [s.span_id, s]))
  const nodes = new Map<string, TreeNode>(
    spans.map((s) => [s.span_id, { span: s, depth: 0, children: [] }]),
  )
  const roots: TreeNode[] = []

  for (const s of spans) {
    const node = nodes.get(s.span_id)!
    const parent = s.parent_span_id ? nodes.get(s.parent_span_id) : undefined
    // A cycle would hang the walk below. It cannot come from mcpd's own ingest,
    // but the parent id is producer-supplied, so the guard is cheap and the
    // alternative is a frozen tab.
    if (parent && parent.span.span_id !== s.span_id && !isAncestor(byId, s.span_id, parent.span)) {
      parent.children.push(node)
    } else {
      roots.push(node)
    }
  }

  const setDepth = (n: TreeNode, d: number) => {
    n.depth = d
    for (const c of n.children) setDepth(c, d + 1)
  }
  for (const r of roots) setDepth(r, 0)
  return roots
}

/** Whether `candidate` already sits under `id`, walking up from the candidate. */
function isAncestor(byId: Map<string, Span>, id: string, candidate: Span): boolean {
  let cur: Span | undefined = candidate
  const seen = new Set<string>()
  while (cur && !seen.has(cur.span_id)) {
    if (cur.span_id === id) return true
    seen.add(cur.span_id)
    cur = cur.parent_span_id ? byId.get(cur.parent_span_id) : undefined
  }
  return false
}

/** Every descendant id of a span, deepest included. */
export function descendantIds(node: TreeNode): string[] {
  const out: string[] = []
  const walk = (n: TreeNode) => {
    for (const c of n.children) {
      out.push(c.span.span_id)
      walk(c)
    }
  }
  walk(node)
  return out
}

export function indexNodes(roots: TreeNode[]): Map<string, TreeNode> {
  const map = new Map<string, TreeNode>()
  const walk = (n: TreeNode) => {
    map.set(n.span.span_id, n)
    n.children.forEach(walk)
  }
  roots.forEach(walk)
  return map
}

/**
 * The rows to draw: the tree, minus anything under a collapsed span, minus
 * anything a search dropped.
 *
 * `visible` is the search's answer, and is undefined when nothing is being
 * searched for. A collapsed span still counts the children it is hiding, so
 * the row can say how many are folded away rather than looking like a leaf.
 */
export function flatten(
  roots: TreeNode[],
  collapsed: ReadonlySet<string>,
  visible?: ReadonlySet<string>,
): Row[] {
  // Counted once for the whole tree, post-order. Walking each row's subtree to
  // count it is quadratic in the depth, and a run with a few thousand steps in
  // a deep chain is exactly where a waterfall is worth having.
  const counted = new Map<string, number>()
  const count = (n: TreeNode): number => {
    let total = 0
    for (const c of n.children) {
      if (visible && !visible.has(c.span.span_id)) continue
      total += 1 + count(c)
    }
    counted.set(n.span.span_id, total)
    return total
  }
  for (const r of roots) count(r)

  const out: Row[] = []
  const walk = (n: TreeNode, parentId?: string) => {
    if (visible && !visible.has(n.span.span_id)) return
    const kids = visible ? n.children.filter((c) => visible.has(c.span.span_id)) : n.children
    out.push({
      span: n.span,
      depth: n.depth,
      hasChildren: kids.length > 0,
      descendants: counted.get(n.span.span_id) ?? 0,
      parentId,
    })
    if (collapsed.has(n.span.span_id)) return
    for (const c of kids) walk(c, n.span.span_id)
  }
  for (const r of roots) walk(r)
  return out
}

/** The kinds present, in the order they first appear, for the legend. */
export function kindsPresent(rows: Row[]): string[] {
  const seen: string[] = []
  for (const r of rows) if (!seen.includes(r.span.kind)) seen.push(r.span.kind)
  return seen
}

/* ------------------------------------------------------------- timing --- */

export interface Extent {
  /** Epoch milliseconds of the earliest start in the run. */
  t0: number
  /** Milliseconds from `t0` to the latest end, never less than one. */
  total: number
  /** True while at least one span has started and not ended. */
  live: boolean
  /**
   * The clock this window was measured against.
   *
   * Carried so a bar is drawn against the same instant the axis was scaled to.
   * Reading the clock again per row let a running bar grow past the end of an
   * axis computed a moment earlier, which is a bar that overflows its own
   * chart by however long the render took.
   */
  now: number
}

/**
 * The window every bar is laid out against.
 *
 * A span that has not ended is measured to now rather than to its start, so a
 * running step does not draw a zero-length bar and, worse, does not make the
 * whole axis end before the work does.
 */
export function extentOf(spans: Span[], now = Date.now()): Extent {
  let t0 = Infinity
  let t1 = -Infinity
  let live = false
  for (const s of spans) {
    const a = new Date(s.start_time).getTime()
    if (Number.isNaN(a)) continue
    const running = !s.end_time && !s.is_event
    if (running) live = true
    const b = s.end_time ? new Date(s.end_time).getTime() : running ? now : a
    if (a < t0) t0 = a
    if (b > t1) t1 = b
  }
  if (!Number.isFinite(t0)) return { t0: now, total: 1, live: false, now }
  return { t0, total: Math.max(1, t1 - t0), live, now }
}

export interface Bar {
  /** Milliseconds from the run's start to this span's start. */
  start: number
  /** How long the span ran, in milliseconds, measured to now while it is open. */
  length: number
  running: boolean
}

/** Where a span's bar starts and how long it runs, in milliseconds from `t0`. */
export function barOf(span: Span, ext: Extent, now = ext.now): Bar {
  const a = new Date(span.start_time).getTime()
  const from = Number.isNaN(a) ? ext.t0 : a
  const running = !span.end_time && !span.is_event
  const to = span.end_time ? new Date(span.end_time).getTime() : running ? now : from
  const end = Number.isNaN(to) ? from : to
  return { start: from - ext.t0, length: Math.max(0, end - from), running }
}

/**
 * Time inside a span that none of its children accounts for.
 *
 * The union of the children, not their sum: concurrent children overlap, and
 * summing them would report a parent as having negative time of its own. Both
 * ends are clamped into the parent, because a child whose clock ran ahead must
 * not subtract time the parent never spent.
 */
export function selfTimeNs(span: Span, spans: Span[]): number | null {
  if (!span.end_time) return null
  const start = new Date(span.start_time).getTime()
  const end = new Date(span.end_time).getTime()
  if (Number.isNaN(start) || Number.isNaN(end)) return null

  const kids = spans
    .filter((k) => k.parent_span_id === span.span_id && k.end_time)
    .map((k) => [
      Math.max(start, new Date(k.start_time).getTime()),
      Math.min(end, new Date(k.end_time!).getTime()),
    ])
    .filter(([a, b]) => b > a)
    .sort((a, b) => a[0] - b[0])

  if (!kids.length) return null

  let covered = 0
  let cursor = start
  for (const [a, b] of kids) {
    const from = Math.max(a, cursor)
    if (b > from) {
      covered += b - from
      cursor = b
    }
  }
  return Math.max(0, end - start - covered) * 1e6
}

/* ------------------------------------------------------------- search --- */

export interface SearchResult {
  /** Every span to draw: the matches, their ancestors and their subtrees. */
  visible: Set<string>
  /** Spans that matched, however they matched. */
  matched: Set<string>
  /** Spans whose only match is inside a payload, an attribute or an id. */
  payloadOnly: Set<string>
  /** How many spans matched. */
  count: number
}

/** How much of one payload is searched. */
const HAYSTACK_CAP = 200_000

function payloadText(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  try {
    const s = JSON.stringify(v)
    return s.length > HAYSTACK_CAP ? s.slice(0, HAYSTACK_CAP) : s
  } catch {
    // A payload with a cycle in it came from a producer, not from mcpd. It is
    // still worth searching what can be read of it rather than dropping the
    // span out of every search.
    return String(v)
  }
}

/**
 * Everything about a span that is not its name, lowercased once.
 *
 * Built per trace and cached by the caller: it is the expensive half of the
 * search, and rebuilding it on every keystroke is what makes an in-trace search
 * feel slow on a run with large prompts.
 */
export function buildHaystack(spans: Span[]): Map<string, string> {
  const out = new Map<string, string>()
  for (const s of spans) {
    const c = s.correlation
    out.set(
      s.span_id,
      [
        s.span_id,
        s.trace_id,
        s.kind,
        s.status,
        s.model ?? '',
        s.provider ?? '',
        s.entity_name ?? '',
        (s.tags ?? []).join(' '),
        s.error?.message ?? '',
        s.error?.kind ?? '',
        c?.session_id ?? '',
        c?.thread_id ?? '',
        c?.run_id ?? '',
        c?.request_id ?? '',
        c?.user_id ?? '',
        c?.resource_id ?? '',
        c?.environment ?? '',
        c?.service_name ?? '',
        payloadText(s.input),
        payloadText(s.output),
        payloadText(s.attributes),
        payloadText(s.metadata),
      ]
        .join(' ')
        .toLowerCase(),
    )
  }
  return out
}

/**
 * Filter the tree without breaking it.
 *
 * A match keeps its ancestors, so a hit six levels down is still reachable, and
 * its whole subtree, so the step you found still shows what it did. Both are
 * what stop a filtered waterfall from reading as a flat list of unrelated rows.
 *
 * `payloadOnly` is the answer to "why is this row here": the query is nowhere in
 * the name, so without the mark the row would look like an ancestor kept for
 * context rather than a match of its own.
 */
export function searchTree(
  roots: TreeNode[],
  nodes: Map<string, TreeNode>,
  haystack: Map<string, string>,
  query: string,
): SearchResult | null {
  const q = query.trim().toLowerCase()
  if (!q) return null

  const matched = new Set<string>()
  const payloadOnly = new Set<string>()
  for (const node of nodes.values()) {
    const id = node.span.span_id
    const inName = node.span.name.toLowerCase().includes(q)
    const inRest = (haystack.get(id) ?? '').includes(q)
    if (inName || inRest) {
      matched.add(id)
      if (!inName) payloadOnly.add(id)
    }
  }

  const parentOf = new Map<string, string>()
  const walk = (n: TreeNode) => {
    for (const c of n.children) {
      parentOf.set(c.span.span_id, n.span.span_id)
      walk(c)
    }
  }
  for (const r of roots) walk(r)

  const visible = new Set<string>()
  for (const id of matched) {
    visible.add(id)
    const node = nodes.get(id)
    if (node) for (const d of descendantIds(node)) visible.add(d)
    let p = parentOf.get(id)
    const guard = new Set<string>()
    while (p && !guard.has(p)) {
      guard.add(p)
      visible.add(p)
      p = parentOf.get(p)
    }
  }

  return { visible, matched, payloadOnly, count: matched.size }
}

/* -------------------------------------------------------- token limit --- */

/**
 * The attribute keys a producer records a stop reason under.
 *
 * mcpd does not model one: `attributes` is an open blob the daemon never
 * interprets, so the interface reads the names the SDKs in the field actually
 * write. Nothing here is inferred. A call is marked only where the producer
 * said so in one of these fields, and the panel quotes the field it read.
 */
const STOP_KEYS = ['finish_reason', 'finishReason', 'stop_reason', 'stopReason']

/** Stop reasons that mean the model ran out of room, across providers. */
const LIMIT_REASONS = new Set([
  'length',
  'max_tokens',
  'max_output_tokens',
  'maxtokens',
  'token_limit',
  'model_length',
])

export interface StopReason {
  /** The key the producer used, so the panel can quote it rather than paraphrase. */
  key: string
  value: string
  /** True when the value means the output was cut off at the limit. */
  atLimit: boolean
}

export function stopReason(span: Span): StopReason | null {
  const a = span.attributes
  if (a == null || typeof a !== 'object' || Array.isArray(a)) return null
  const rec = a as Record<string, unknown>
  for (const key of STOP_KEYS) {
    const v = rec[key]
    if (typeof v === 'string' && v.length) {
      return { key, value: v, atLimit: LIMIT_REASONS.has(v.toLowerCase()) }
    }
  }
  return null
}
