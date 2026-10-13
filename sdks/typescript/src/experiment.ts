/**
 * The experiment runner (SDK contract 5.2).
 *
 * The daemon never calls the agent under test. This does, in your process, once
 * per dataset case inside a root span that names the experiment and the case,
 * and then asks the daemon to score what was recorded.
 */

import { contextReady, contextStore } from './context.js'
import type { DatasetItem, Experiment, ExperimentResults, ScorerConfig } from './generated.js'
import { runInSpan } from './span.js'
import type { McpdSpan } from './span.js'
import type { TraceOptions } from './client.js'
import { McpdError } from './transport.js'

/** The attribute the daemon reads to find the case a run answered. */
export const DATASET_ITEM_ATTRIBUTE = 'mcpd.dataset_item_id'

/** The most cases the daemon answers in one page. */
const MAX_CASES = 5000

/** How `runExperiment` names, scores and paces an experiment. */
export interface RunExperimentOptions {
  /** The experiment's name, and the name of each run's root span. Defaults to the dataset id. */
  name?: string
  /** The scorers, as `POST /v1/api/score` takes them. Empty runs the code scorers that need no settings. */
  scorers?: string[]
  /** Settings for the scorers. Each case's expected answer is filled in by the daemon. */
  config?: ScorerConfig
  /** How many cases run at once. 4 by default. */
  concurrency?: number
  /** Anything you want to keep with the experiment, such as the prompt version under test. */
  metadata?: unknown
}

/** The code under test: called with each case's input, and what it returns is the answer. */
export type ExperimentTarget = (input: unknown, item: DatasetItem) => unknown

/** What the runner needs from a client. `Mcpd` is one. */
export interface ExperimentHost {
  readonly enabled: boolean
  readonly project: string
  trace(name: string, options?: TraceOptions): McpdSpan
  flush(): Promise<boolean>
  createExperiment(experiment: Experiment): Promise<Experiment>
  listDatasetItems(datasetId: string, options?: { limit?: number }): Promise<DatasetItem[]>
  scoreExperiment(experimentId: string, scorers: string[], config?: ScorerConfig): Promise<unknown>
  experimentResults(experimentId: string): Promise<ExperimentResults>
  diagnose(operation: string, error: unknown): void
}

/** Run `target` over every case of a dataset, score the answers and return the results. */
export async function runExperiment(
  host: ExperimentHost,
  datasetId: string,
  target: ExperimentTarget,
  options: RunExperimentOptions = {},
): Promise<ExperimentResults> {
  if (!host.enabled) {
    throw new McpdError('runExperiment needs an enabled client: no run would reach the daemon to be scored')
  }
  const concurrency = options.concurrency ?? 4
  if (!Number.isInteger(concurrency) || concurrency < 1) {
    throw new McpdError(`runExperiment concurrency must be a whole number of at least 1, got ${concurrency}`)
  }
  const draft: Experiment = {
    experimentId: '',
    name: options.name ?? datasetId,
    datasetId,
    status: 'running',
    startedAt: new Date().toISOString(),
    itemCount: 0,
    correlation: { project: host.project },
  }
  if (options.metadata !== undefined) draft.metadata = options.metadata

  let experiment: Experiment | undefined
  try {
    experiment = await host.createExperiment(draft)
    const items = await host.listDatasetItems(datasetId, { limit: MAX_CASES })

    // A fixed set of workers pulls the next case, so at most `concurrency`
    // targets are in flight however long the dataset is.
    const running = experiment
    let next = 0
    const worker = async (): Promise<void> => {
      while (next < items.length) {
        await runCase(host, running, items[next++]!, target)
      }
    }
    await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker))

    if (!(await host.flush())) {
      throw new McpdError('the runs did not reach the daemon before the flush deadline, so none was scored')
    }
    await host.scoreExperiment(running.experimentId, options.scorers ?? [], options.config)
    const results = await host.experimentResults(running.experimentId)
    const summary: Record<string, number> = {}
    for (const scorer of results.scorers) summary[scorer.scorerId] = scorer.mean
    await host.createExperiment({
      ...running,
      status: 'succeeded',
      finishedAt: new Date().toISOString(),
      itemCount: items.length,
      summary,
    })
    return results
  } catch (error) {
    if (experiment !== undefined) {
      try {
        await host.createExperiment({
          ...experiment,
          status: 'failed',
          finishedAt: new Date().toISOString(),
          error: error instanceof Error ? error.message : String(error),
        })
      } catch (recording) {
        host.diagnose('runExperiment could not record the failed status', recording)
      }
    }
    throw error
  }
}

/** One case: a root span around the target. A target that throws fails its own span only. */
async function runCase(
  host: ExperimentHost,
  experiment: Experiment,
  item: DatasetItem,
  target: ExperimentTarget,
): Promise<void> {
  await contextReady()
  const span = host.trace(experiment.name, {
    kind: 'agent_run',
    // A sampled-out case would be a case with no answer.
    sampled: true,
    input: item.input,
    attributes: { [DATASET_ITEM_ATTRIBUTE]: item.itemId },
    correlation: { experimentId: experiment.experimentId },
  })
  try {
    await contextStore().run(span, () =>
      runInSpan(span, async (run) => {
        const output = await target(item.input, item)
        if (output !== undefined) run.setOutput(output)
      }),
    )
  } catch {
    // Recorded on the span by `runInSpan`; the next case still runs.
  }
}
