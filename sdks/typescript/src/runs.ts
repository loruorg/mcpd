/**
 * The run surface of the daemon, as a client: definitions, runs, events, and
 * the worker protocol `Worker` drives.
 *
 * These calls throw on failure, like the read API, because a run you asked
 * for and did not get is news. A worker call refused with a 409 throws a
 * `StepConflictError` carrying the daemon's closed reason, which is what lets
 * a worker tell a fence from its own landed call.
 */

import type {
  ClaimRequest,
  ClaimResponse,
  Correlation,
  Definition,
  DefinitionDocument,
  DefinitionVersion,
  EventReceipt,
  LeaseState,
  Run,
  RunDetail,
  RunEvent,
  RunEventPage,
  RunListResponse,
  RunStatus,
  StepAck,
  StepConflict,
  StepFailure as StepFailureBody,
  StepResult,
  WorkerRegistered,
  WorkerRegistration,
} from './generated.js'
import type { QueryValue, Transport } from './transport.js'
import { McpdError, fromWire } from './transport.js'

/** A worker call the daemon refused with one of the four closed reasons. */
export class StepConflictError extends McpdError {
  /** The daemon's answer in full. */
  readonly conflict: StepConflict

  constructor(conflict: StepConflict, cause: McpdError) {
    super(`${conflict.reason}: ${conflict.message}`, {
      status: 409,
      retryable: false,
      body: cause.body,
      cause,
    })
    this.name = 'StepConflictError'
    this.conflict = conflict
  }
}

/** Rethrow a 409 as a typed conflict, and anything else unchanged. */
function typed<T>(promise: Promise<T>): Promise<T> {
  return promise.catch((error: unknown) => {
    if (error instanceof McpdError && error.status === 409 && error.body) {
      try {
        const parsed = fromWire<StepConflict>(JSON.parse(error.body))
        if (parsed && typeof parsed === 'object' && 'reason' in parsed) {
          throw new StepConflictError(parsed, error)
        }
      } catch (inner) {
        if (inner instanceof StepConflictError) throw inner
      }
    }
    throw error
  })
}

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, ms)
  })

/** A run that ended without succeeding, thrown by `wait` with the run attached. */
export class RunFailedError extends McpdError {
  /** `failed`, `timed_out` or `cancelled`. (`status` is the HTTP status, unused here.) */
  readonly runStatus: RunStatus
  /** The run as it ended. */
  readonly run: Run

  constructor(run: Run) {
    super(`run ${run.runId} ${run.status}${run.error ? `: ${run.error}` : ''}`, { retryable: false })
    this.name = 'RunFailedError'
    this.runStatus = run.status
    this.run = run
  }
}

/** What `wait` answers: the run as it ended, with its output to hand. */
export interface RunResult {
  /** Always `succeeded`; any other ending throws. */
  status: RunStatus
  /** What the definition's `output` template rendered. */
  output: unknown
  /** The priced usage the steps reported, in USD. */
  costUsd: number
  /** How many attempts the run spent. */
  stepsUsed: number
  /** The trace every step of the run landed in. */
  traceId: string
  /** The run in full. */
  run: Run
}

/** What every run call may name. */
export interface Scoped {
  /** Which project. Needed only when the key reaches several. */
  project?: string
}

/** What `runs.start` takes. */
export interface StartRunOptions extends Scoped {
  /** Which definition to run. A `Workflow` may be passed in its place. */
  definitionId: string
  /** Which version. Absent pins the highest stored version at start time. */
  version?: number
  /** The run's input, validated against the definition's `input_schema` when it has one. */
  input?: unknown
  /** Session, user and the other correlation fields the run's spans carry. */
  correlation?: Partial<Correlation>
  /** Unique per project while held: a retry of this call returns the same run. */
  idempotencyKey?: string
}

/** Definitions: the immutable, versioned documents the daemon walks. */
export class DefinitionsApi {
  readonly #transport: Transport
  readonly #project: string

  constructor(transport: Transport, project: string) {
    this.#transport = transport
    this.#project = project
  }

  /** Store one version of a definition, and read the version back. */
  async put(doc: DefinitionDocument, options: Scoped = {}): Promise<DefinitionVersion> {
    return await this.#transport.post<DefinitionVersion>(
      `/v1/api/definitions${query({ project: options.project ?? this.#project })}`,
      doc,
    )
  }

  /** One definition: the latest version, unless `version` names one. */
  async get(id: string, options: { version?: number } & Scoped = {}): Promise<Definition> {
    return await this.#transport.get<Definition>(`/v1/api/definitions/${encodeURIComponent(id)}`, {
      project: options.project ?? this.#project,
      version: options.version,
    })
  }

  /** Every stored version in the project, or every version of one id. */
  async list(options: { id?: string } & Scoped = {}): Promise<Definition[]> {
    return await this.#transport.get<Definition[]>('/v1/api/definitions', {
      project: options.project ?? this.#project,
      id: options.id,
    })
  }
}

/** Runs: start, read, watch, cancel, signal. */
export class RunsApi {
  readonly #transport: Transport
  readonly #project: string

  constructor(transport: Transport, project: string) {
    this.#transport = transport
    this.#project = project
  }

  /** Start a run. The response is the run as stored, with its trace id. */
  async start(options: StartRunOptions): Promise<Run> {
    const body: Record<string, unknown> = {
      project: options.project ?? this.#project,
      definitionId: options.definitionId,
    }
    if (options.version !== undefined) body['version'] = options.version
    if (options.input !== undefined) body['input'] = options.input
    if (options.correlation !== undefined) {
      body['correlation'] = { project: options.project ?? this.#project, ...options.correlation }
    }
    if (options.idempotencyKey !== undefined) body['idempotencyKey'] = options.idempotencyKey
    return await this.#transport.post<Run>('/v1/api/runs', body)
  }

  /** One run and everything durable about it: its journal, deadlines and waits. */
  async get(runId: string): Promise<RunDetail> {
    return await this.#transport.get<RunDetail>(`/v1/api/runs/${encodeURIComponent(runId)}`)
  }

  /** Runs in the project, most recently updated first. */
  async list(
    options: { status?: RunStatus; definitionId?: string; limit?: number; cursor?: string } & Scoped = {},
  ): Promise<RunListResponse> {
    return await this.#transport.get<RunListResponse>('/v1/api/runs', {
      project: options.project ?? this.#project,
      status: options.status,
      definitionId: options.definitionId,
      limit: options.limit,
      cursor: options.cursor,
    })
  }

  /** End a run and every child run under it. Cancelling an ended run changes nothing. */
  async cancel(runId: string): Promise<Run> {
    return await this.#transport.post<Run>(`/v1/api/runs/${encodeURIComponent(runId)}/cancel`, {})
  }

  /** A page of the run's journal, from `after` exclusive. */
  async events(runId: string, options: { after?: number; limit?: number } = {}): Promise<RunEventPage> {
    return await this.#transport.get<RunEventPage>(
      `/v1/api/runs/${encodeURIComponent(runId)}/events`,
      { after: options.after, limit: options.limit },
    )
  }

  /**
   * Every event of a run as it happens, from `after` exclusive, until the run
   * ends. A client that stops and starts again passes the last `seq` it saw
   * and misses nothing that was committed.
   */
  async *watch(
    runId: string,
    options: { after?: number; pollMs?: number } = {},
  ): AsyncGenerator<RunEvent, void, undefined> {
    let after = options.after ?? -1
    const pollMs = options.pollMs ?? 500
    for (;;) {
      const page = await this.events(runId, { after, limit: 500 })
      let ended = false
      for (const event of page.events) {
        after = Math.max(after, Number(event.seq))
        yield event
        if (event.kind === 'run_ended') ended = true
      }
      if (ended) return
      if (page.hasMore) continue
      await sleep(pollMs)
    }
  }

  /**
   * Wait for a run to end and return it with its output. A run that ended
   * `failed`, `timed_out` or `cancelled` throws `RunFailedError` carrying the
   * run; a run still going after `timeoutMs` throws a plain `McpdError`.
   */
  async wait(
    runId: string,
    options: { after?: number; timeoutMs?: number; pollMs?: number } = {},
  ): Promise<RunResult> {
    // Written against the page rather than over `watch`: a paused generator
    // cannot be closed while it is asleep inside a poll, so a timeout here
    // would leak a loop that keeps polling a run nobody is waiting for.
    const deadline = options.timeoutMs === undefined ? undefined : Date.now() + options.timeoutMs
    const pollMs = options.pollMs ?? 500
    let after = options.after ?? -1
    for (;;) {
      const page = await this.events(runId, { after, limit: 500 })
      let ended = false
      for (const event of page.events) {
        after = Math.max(after, Number(event.seq))
        if (event.kind === 'run_ended') ended = true
      }
      if (ended) break
      if (page.hasMore) continue
      if (deadline !== undefined && Date.now() >= deadline) {
        throw new McpdError(`run ${runId} is still running after ${options.timeoutMs}ms`, { retryable: true })
      }
      await sleep(deadline === undefined ? pollMs : Math.min(pollMs, Math.max(0, deadline - Date.now())))
    }
    const detail = await this.get(runId)
    const run = detail.run
    if (run.status !== 'succeeded') throw new RunFailedError(run)
    return {
      status: run.status,
      output: run.output,
      costUsd: run.costUsd,
      stepsUsed: run.stepsUsed,
      traceId: run.traceId,
      run,
    }
  }

  /** `start`, then `wait`. */
  async startAndWait(
    options: StartRunOptions & { timeoutMs?: number; pollMs?: number },
  ): Promise<RunResult> {
    const { timeoutMs, pollMs, ...start } = options
    const run = await this.start(start)
    return await this.wait(run.runId, { timeoutMs, pollMs })
  }

  /** Deliver an event to one run through its own open wait. */
  async signal(
    runId: string,
    options: { event: string; payload?: unknown; eventId?: string },
  ): Promise<EventReceipt> {
    const body: Record<string, unknown> = { event: options.event }
    if (options.payload !== undefined) body['payload'] = options.payload
    if (options.eventId !== undefined) body['eventId'] = options.eventId
    return await this.#transport.post<EventReceipt>(
      `/v1/api/runs/${encodeURIComponent(runId)}/signal`,
      body,
    )
  }
}

/** Events: the door a webhook or an approval console posts to. */
export class EventsApi {
  readonly #transport: Transport
  readonly #project: string

  constructor(transport: Transport, project: string) {
    this.#transport = transport
    this.#project = project
  }

  /**
   * Route an event by name and key to the run waiting for it. Send your own
   * `eventId` from any handler that can be retried, such as a webhook: a
   * repeat of a supplied id is answered `duplicate` and writes nothing.
   */
  async publish(
    options: { event: string; key: string; payload?: unknown; eventId?: string } & Scoped,
  ): Promise<EventReceipt> {
    const body: Record<string, unknown> = {
      project: options.project ?? this.#project,
      event: options.event,
      key: options.key,
    }
    if (options.payload !== undefined) body['payload'] = options.payload
    if (options.eventId !== undefined) body['eventId'] = options.eventId
    return await this.#transport.post<EventReceipt>('/v1/api/events', body)
  }
}

/** The worker protocol, call for call. `Worker` drives it; a bare script can too. */
export class StepsApi {
  readonly #transport: Transport
  readonly #project: string

  constructor(transport: Transport, project: string) {
    this.#transport = transport
    this.#project = project
  }

  /** Announce a worker and store what it serves. */
  async register(
    options: { workerId: string; definitions: DefinitionDocument[] } & Scoped,
  ): Promise<WorkerRegistered> {
    const body: WorkerRegistration = {
      project: options.project ?? this.#project,
      workerId: options.workerId,
      definitions: options.definitions,
    }
    return await this.#transport.post<WorkerRegistered>('/v1/api/workers/register', body)
  }

  /** Offer up to `max` runnable steps, waiting up to `waitMs` for one. */
  async claim(
    options: { workerId: string; max: number; waitMs: number; runId?: string } & Scoped,
  ): Promise<ClaimResponse> {
    const body: ClaimRequest = {
      project: options.project ?? this.#project,
      workerId: options.workerId,
      max: options.max,
      waitMs: options.waitMs,
    }
    if (options.runId !== undefined) body.runId = options.runId
    return await this.#transport.post<ClaimResponse>('/v1/api/steps/claim', body, { waitMs: options.waitMs })
  }

  /** Declare that the body is running. Required under `at_most_once`. */
  async started(leaseId: string): Promise<LeaseState> {
    return await typed(
      this.#transport.post<LeaseState>(`/v1/api/steps/${encodeURIComponent(leaseId)}/started`, {}),
    )
  }

  /** Extend the lease. The only evidence the daemon has that a worker is alive. */
  async heartbeat(leaseId: string): Promise<LeaseState> {
    return await typed(
      this.#transport.post<LeaseState>(`/v1/api/steps/${encodeURIComponent(leaseId)}/heartbeat`, {}),
    )
  }

  /** Finish the step. */
  async complete(leaseId: string, result: StepResult): Promise<StepAck> {
    return await typed(
      this.#transport.post<StepAck>(`/v1/api/steps/${encodeURIComponent(leaseId)}/complete`, result),
    )
  }

  /** Fail the step, with the class that decides whether it is retried. */
  async fail(leaseId: string, failure: StepFailureBody): Promise<StepAck> {
    return await typed(
      this.#transport.post<StepAck>(`/v1/api/steps/${encodeURIComponent(leaseId)}/fail`, failure),
    )
  }
}

function query(params: Record<string, QueryValue>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === '') continue
    p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}
