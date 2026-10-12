/**
 * The authoring surface for durable workflows.
 *
 * A workflow is data the daemon holds and bodies the daemon never sees. The
 * builder produces both from one file: `document()` is the definition the
 * daemon walks, in the shape `spec/definition.schema.json` describes, and the
 * handlers stay in this process for a `Worker` to serve. There is no YAML to
 * keep in step with the code, because the code is where the graph is written.
 *
 * Every construct of the run contract has a method here. A branch, a foreach
 * body and a loop body are one claimable step each; a multi-step body is a
 * `subworkflow`.
 */

import type {
  AttemptStatus,
  Backoff,
  DefinitionDocument,
  Effect,
  OnError,
  StepErrorClass,
  WaitTimeout,
} from './generated.js'
import type { StepContext } from './worker.js'

// ---------------------------------------------------------------------------
// Predicates and templates
// ---------------------------------------------------------------------------

/** The comparison a guard makes. */
export type PredicateOp = 'eq' | 'ne' | 'lt' | 'lte' | 'gt' | 'gte' | 'in' | 'exists' | 'matches'

/**
 * A guard the daemon evaluates. A leaf compares a path against a value, and
 * `all`, `any` and `not` combine leaves. A path reads `input`, `steps.<id>`,
 * `run`, and `item`, `index` or `iter.index` where they apply.
 */
export type Predicate =
  | { path: string; op: PredicateOp; value?: unknown }
  | { all: Predicate[] }
  | { any: Predicate[] }
  | { not: Predicate }

/** A leaf predicate: `when('steps.order.output.amount_usd', 'gt', 100)`. */
export function when(path: string, op: PredicateOp, value?: unknown): Predicate {
  return value === undefined ? { path, op } : { path, op, value }
}

/** Every predicate must hold. */
export function all(...predicates: Predicate[]): Predicate {
  return { all: predicates }
}

/** At least one predicate must hold. */
export function any(...predicates: Predicate[]): Predicate {
  return { any: predicates }
}

/** The predicate must not hold. */
export function not(predicate: Predicate): Predicate {
  return { not: predicate }
}

/**
 * A retry policy. `maxAttempts` counts every attempt including the first, so
 * the default of one means no retry.
 */
export interface RetryPolicy {
  maxAttempts?: number
  backoff?: Backoff
  initialMs?: number
  maxMs?: number
}

/** What every claimable step accepts. */
export interface StepOptions {
  /** A guard. False writes the step as `skipped` and the run moves on. */
  when?: Predicate
  /** The input template. Default: the run's input. `${path}` references are rendered per attempt. */
  input?: unknown
  /** What a duplicated body would cost. Default `at_least_once`. */
  effect?: Effect
  /** One attempt's own ceiling, heartbeats or not. */
  stepTimeoutMs?: number
  /** How failures classed `retryable` are retried. */
  retry?: RetryPolicy
  /** What happens after the retries. Default `fail_run`. */
  onError?: OnError
}

/** What a step that is not claimable accepts. */
export interface NodeOptions {
  when?: Predicate
  onError?: OnError
}

/**
 * What a body sees of the steps that already ended, keyed by step id. A
 * `parallel` node nests its branches; a `foreach` or `loop` carries `outputs`.
 */
export interface PriorStep {
  status: AttemptStatus
  output?: unknown
  error?: { class?: StepErrorClass; message?: string }
  outputs?: unknown[]
  [branch: string]: unknown
}

/** The `prior` a lease carries. */
export type Prior = Record<string, PriorStep>

/**
 * A step body. Returns the step's output, or throws. A thrown `StepFailure`
 * carries its own class; any other throw is `fatal`.
 */
export type StepHandler<I = unknown, O = unknown> = (
  ctx: StepContext,
  input: I,
  prior: Prior,
) => O | Promise<O>

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

/** A node in the wire shape. Snake case, because the body is opaque JSON the daemon parses. */
type WireNode = Record<string, unknown>

function withCommon(node: WireNode, options: NodeOptions | StepOptions): WireNode {
  if (options.when !== undefined) node['when'] = options.when
  if (options.onError !== undefined) node['on_error'] = options.onError
  return node
}

function claimable(
  kind: 'task' | 'agent',
  id: string,
  options: StepOptions,
): WireNode {
  const node: WireNode = withCommon({ kind, id }, options)
  if (options.input !== undefined) node['input'] = options.input
  if (options.effect !== undefined) node['effect'] = options.effect
  if (options.stepTimeoutMs !== undefined) node['step_timeout_ms'] = options.stepTimeoutMs
  if (options.retry !== undefined) {
    const retry: WireNode = {}
    if (options.retry.maxAttempts !== undefined) retry['max_attempts'] = options.retry.maxAttempts
    if (options.retry.backoff !== undefined) retry['backoff'] = options.retry.backoff
    if (options.retry.initialMs !== undefined) retry['initial_ms'] = options.retry.initialMs
    if (options.retry.maxMs !== undefined) retry['max_ms'] = options.retry.maxMs
    node['retry'] = retry
  }
  return node
}

const ID = /^[a-z][a-z0-9_]*$/

function checkId(id: string): string {
  if (!ID.test(id)) {
    throw new Error(`"${id}" is not a step id; an id matches ^[a-z][a-z0-9_]*$`)
  }
  return id
}

/** What a parallel branch, a foreach body or a loop body may hold: one claimable step. */
export class BranchBuilder {
  readonly nodes: WireNode[] = []
  readonly handlers = new Map<string, StepHandler>()

  /** A branch or body that a worker serves. */
  step<I = unknown, O = unknown>(
    id: string,
    handler: StepHandler<I, O>,
    options: StepOptions = {},
  ): this {
    this.nodes.push(claimable('task', checkId(id), options))
    this.handlers.set(id, handler as StepHandler)
    return this
  }

  /** The same, recorded as an agent turn. */
  agent<I = unknown, O = unknown>(
    id: string,
    handler: StepHandler<I, O>,
    options: StepOptions = {},
  ): this {
    this.nodes.push(claimable('agent', checkId(id), options))
    this.handlers.set(id, handler as StepHandler)
    return this
  }
}

/** The ceilings a workflow declares. */
export interface Limits {
  maxSteps?: number
  runTimeoutMs?: number
  maxCostUsd?: number
}

/** What `workflow()` takes. */
export interface WorkflowOptions {
  /** The definition id, unique within the project. */
  id: string
  /** Name a version to own the number; leave it out and the daemon assigns one. */
  version?: number
  /** A JSON Schema the run's input must satisfy. */
  inputSchema?: unknown
  /** The ceilings. */
  limits?: Limits
}

/**
 * A workflow under construction. Steps run in the order they are added.
 */
export class Workflow {
  readonly id: string
  readonly version: number | undefined
  readonly #nodes: WireNode[] = []
  readonly #handlers = new Map<string, StepHandler>()
  /** The body id a foreach or loop root maps to, so a lease's key finds its handler. */
  readonly #bodyOf = new Map<string, string>()
  readonly #inputSchema: unknown
  readonly #limits: Limits | undefined
  #output: unknown

  constructor(options: WorkflowOptions) {
    this.id = checkId(options.id)
    this.version = options.version
    this.#inputSchema = options.inputSchema
    this.#limits = options.limits
  }

  /** A claimable step a worker serves. */
  step<I = unknown, O = unknown>(
    id: string,
    handler: StepHandler<I, O>,
    options: StepOptions = {},
  ): this {
    this.#nodes.push(claimable('task', checkId(id), options))
    this.#handlers.set(id, handler as StepHandler)
    return this
  }

  /** A claimable step recorded as an agent turn: your own loop, however many turns, as one durable step. */
  agent<I = unknown, O = unknown>(
    id: string,
    handler: StepHandler<I, O>,
    options: StepOptions = {},
  ): this {
    this.#nodes.push(claimable('agent', checkId(id), options))
    this.#handlers.set(id, handler as StepHandler)
    return this
  }

  /** A delay. No process is alive during it. */
  sleep(id: string, options: { ms: number } & NodeOptions): this {
    this.#nodes.push(withCommon({ kind: 'sleep', id: checkId(id), ms: options.ms }, options))
    return this
  }

  /** A delay until an instant, rendered from a template such as `${input.billing_date}`. */
  sleepUntil(id: string, options: { at: string } & NodeOptions): this {
    this.#nodes.push(withCommon({ kind: 'sleep_until', id: checkId(id), at: options.at }, options))
    return this
  }

  /**
   * A pause until an event arrives. The daemon holds the deadline and the
   * mailbox; the key is rendered per run, so `${input.order_id}` addresses this
   * run and no other.
   */
  waitFor(
    id: string,
    options: {
      event: string
      key: string
      timeoutMs: number
      acceptParked?: boolean
      onTimeout?: WaitTimeout
    } & NodeOptions,
  ): this {
    const node: WireNode = withCommon(
      {
        kind: 'wait_event',
        id: checkId(id),
        event: options.event,
        key: options.key,
        timeout_ms: options.timeoutMs,
      },
      options,
    )
    if (options.acceptParked !== undefined) node['accept_parked'] = options.acceptParked
    if (options.onTimeout !== undefined) node['on_timeout'] = options.onTimeout
    this.#nodes.push(node)
    return this
  }

  /** Named branches, each one claimable step, all on disk before any is offered. */
  parallel(id: string, build: (b: BranchBuilder) => void, options: NodeOptions = {}): this {
    const b = new BranchBuilder()
    build(b)
    this.#nodes.push(withCommon({ kind: 'parallel', id: checkId(id), branches: b.nodes }, options))
    for (const [branch, handler] of b.handlers) this.#handlers.set(branch, handler)
    return this
  }

  /**
   * One claimable step per element of the array at `over`. The body's input
   * template can read `item` and `index`; afterwards `steps.<id>.outputs` is
   * the array of outputs in item order.
   */
  foreach(
    id: string,
    options: { over: string; concurrency?: number; maxItems: number } & NodeOptions,
    build: (b: BranchBuilder) => void,
  ): this {
    const b = new BranchBuilder()
    build(b)
    const body = b.nodes[0]
    if (b.nodes.length !== 1 || body === undefined) {
      throw new Error(`foreach "${id}" needs exactly one body step; use a subworkflow for more`)
    }
    const node: WireNode = withCommon(
      { kind: 'foreach', id: checkId(id), over: options.over, body, max_items: options.maxItems },
      options,
    )
    if (options.concurrency !== undefined) node['concurrency'] = options.concurrency
    this.#nodes.push(node)
    for (const [bodyId, handler] of b.handlers) {
      this.#handlers.set(bodyId, handler)
      this.#bodyOf.set(id, bodyId)
    }
    return this
  }

  /**
   * A bounded loop. `until` is evaluated after each iteration and reads
   * `steps.<body id>` for that iteration; `iter.index` is the iteration number.
   */
  loop(
    id: string,
    options: { maxIterations: number; until?: Predicate } & NodeOptions,
    build: (b: BranchBuilder) => void,
  ): this {
    const b = new BranchBuilder()
    build(b)
    const body = b.nodes[0]
    if (b.nodes.length !== 1 || body === undefined) {
      throw new Error(`loop "${id}" needs exactly one body step; use a subworkflow for more`)
    }
    const node: WireNode = withCommon(
      { kind: 'loop', id: checkId(id), body, max_iterations: options.maxIterations },
      options,
    )
    if (options.until !== undefined) node['until'] = options.until
    this.#nodes.push(node)
    for (const [bodyId, handler] of b.handlers) {
      this.#handlers.set(bodyId, handler)
      this.#bodyOf.set(id, bodyId)
    }
    return this
  }

  /** A child run of another definition. Its output becomes `steps.<id>.output`. */
  subworkflow(
    id: string,
    options: { definition: string; version?: number; input?: unknown } & NodeOptions,
  ): this {
    const node: WireNode = withCommon(
      { kind: 'subworkflow', id: checkId(id), definition: options.definition },
      options,
    )
    if (options.version !== undefined) node['version'] = options.version
    if (options.input !== undefined) node['input'] = options.input
    this.#nodes.push(node)
    return this
  }

  /** The template rendered when the run reaches the end of the graph. */
  output(template: unknown): this {
    this.#output = template
    return this
  }

  /** The definition document the daemon stores and walks. */
  document(): DefinitionDocument {
    const body: WireNode = { steps: this.#nodes }
    if (this.#inputSchema !== undefined) body['input_schema'] = this.#inputSchema
    if (this.#limits !== undefined) {
      const limits: WireNode = {}
      if (this.#limits.maxSteps !== undefined) limits['max_steps'] = this.#limits.maxSteps
      if (this.#limits.runTimeoutMs !== undefined) limits['run_timeout_ms'] = this.#limits.runTimeoutMs
      if (this.#limits.maxCostUsd !== undefined) limits['max_cost_usd'] = this.#limits.maxCostUsd
      body['limits'] = limits
    }
    if (this.#output !== undefined) body['output'] = this.#output
    const doc: DefinitionDocument = { id: this.id, kind: 'workflow', body }
    if (this.version !== undefined) doc.version = this.version
    return doc
  }

  /**
   * The handler a lease's step key addresses. `order`, `checks/fraud`,
   * `notify[3]` and `triage[2]/classify` each resolve to the lexical id that
   * registered the body.
   */
  handlerFor(stepKey: string): StepHandler | undefined {
    const slash = stepKey.indexOf('/')
    const head = slash === -1 ? stepKey : stepKey.slice(0, slash)
    const sub = slash === -1 ? undefined : stepKey.slice(slash + 1)
    const bracket = head.indexOf('[')
    const root = bracket === -1 ? head : head.slice(0, bracket)
    if (sub !== undefined) return this.#handlers.get(sub)
    if (bracket !== -1) return this.#handlers.get(this.#bodyOf.get(root) ?? root)
    return this.#handlers.get(root)
  }
}

/** Start building a workflow. */
export function workflow(options: WorkflowOptions): Workflow {
  return new Workflow(options)
}
