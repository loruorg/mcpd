/**
 * The seven probes, and how to run each one.
 *
 * Shared by every end-to-end suite that drives an SDK, so a language is added,
 * detected and located in exactly one place. Two copies of this table would
 * drift, and the copy that drifts silently reports the language as skipped,
 * which is the one result nobody reads.
 */

import { spawn, spawnSync } from 'node:child_process'
import { existsSync, realpathSync, statSync } from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import process from 'node:process'

export const ROOT = path.resolve(import.meta.dirname, '..', '..')
export const PROBES = path.join(ROOT, 'tests/e2e/probes')

/**
 * The mcpd binary the suites run: `MCPD_BIN` when set, else the one
 * `cargo build -p mcpd` just wrote. That is under `CARGO_TARGET_DIR` when it
 * is set, so a build there is never passed over for an older one in `target/`.
 */
export function mcpdBinary() {
  if (process.env.MCPD_BIN) return existsSync(process.env.MCPD_BIN) ? process.env.MCPD_BIN : undefined
  const dirs = [process.env.CARGO_TARGET_DIR, path.join(ROOT, 'target')].filter(Boolean)
  return dirs
    .flatMap((dir) => ['debug', 'release'].map((profile) => path.resolve(ROOT, dir, profile, 'mcpd')))
    .find(existsSync)
}

/**
 * A port the operating system says is free.
 *
 * Never a fixed number. A hardcoded port picks up whatever else is listening on
 * a shared machine, and a suite that reads someone else's server reports
 * failures that are entirely imaginary.
 */
export function freePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer()
    probe.on('error', reject)
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address()
      probe.close(() => resolve(port))
    })
  })
}

/**
 * Each entry says how to run that language's probe, and how to know whether it
 * can run at all. `detect` returning false is a skip; a probe that runs and
 * fails is a failure.
 *
 * Detection looks on `PATH` first and only then in the fixed locations a
 * development machine happens to use. A detector that names one machine's
 * directories reports every other machine as "no toolchain", which is a skip,
 * and a skip is the one result nobody reads.
 */
export const LANGUAGES = [
  {
    id: 'typescript',
    // Node strips types natively from 22.6, so the probe runs with no build
    // step and no test runner. One fewer thing between the SDK and the proof
    // that it works.
    detect: () => existsSync(path.join(ROOT, 'sdks/typescript/dist/esm/index.js')),
    why: 'sdks/typescript/dist is not built; run `npm ci && npm run build` in sdks/typescript',
    run: (env, opts) => exec('node', ['--experimental-strip-types', 'tests/e2e/probes/probe.ts'], { env, ...opts }),
  },
  {
    id: 'python',
    detect: () => which('python3'),
    why: 'python3 is not on PATH',
    run: (env, opts) =>
      exec('python3', ['tests/e2e/probes/probe.py'], {
        env: { ...env, PYTHONPATH: path.join(ROOT, 'sdks/python') },
        ...opts,
      }),
  },
  {
    id: 'go',
    detect: () => Boolean(goBin()),
    why: 'go is not on PATH',
    run: (env, opts) =>
      exec(goBin(), ['run', '.'], { cwd: PROBES, env: { ...env, GOFLAGS: '-mod=mod' }, ...opts }),
  },
  {
    id: 'elixir',
    detect: () =>
      Boolean(elixir().run) && existsSync(path.join(ROOT, 'sdks/elixir/_build/Elixir.Mcpd.beam')),
    why: 'elixir is not on PATH, or sdks/elixir/_build holds no compiled SDK',
    run: (env, opts) => {
      const { run, bin: elixirBin } = elixir()
      return exec(run, ['-pa', 'sdks/elixir/_build', 'tests/e2e/probes/probe.exs'], {
        env: { ...env, PATH: elixirBin ? `${elixirBin}:${env.PATH}` : env.PATH },
        ...opts,
      })
    },
  },
  {
    id: 'kotlin',
    // The JVM probe is compiled ahead of the run, because kotlinc takes long
    // enough that compiling it inside the harness would dominate the suite.
    // It is recompiled when Probe.kt is newer than the class file: running
    // yesterday's bytecode against today's probe is a pass that means nothing.
    detect: () => Boolean(java()) && kotlinProbeClass(),
    why: 'no JDK, or Probe.kt has changed and kotlinc is not available to recompile it',
    run: (env, opts) =>
      exec(java(), ['-cp', `${KT_OUT}:${KT_SDK}:${kotlinClasspath()}`, 'ProbeKt'], { env, ...opts }),
  },
  {
    id: 'rust',
    detect: () => which('cargo'),
    why: 'cargo is not on PATH',
    run: (env, opts) =>
      exec('cargo', ['run', '--quiet'], {
        cwd: path.join(PROBES, 'rust'),
        env,
        // Long by default because the first run compiles the probe.
        timeout: 600_000,
        ...opts,
      }),
  },
  {
    id: 'curl',
    detect: () => which('curl'),
    why: 'curl is not on PATH',
    run: (env, opts) => exec('bash', ['tests/e2e/probes/probe.sh'], { env, ...opts }),
  },
]

/**
 * Where the Kotlin pieces are.
 *
 * Overridable, because the defaults are one development machine's directories
 * and a machine that has none of them would report Kotlin as "no toolchain",
 * which is a skip, and a skip is the result nobody reads. `MCPD_E2E_KOTLIN_DEPS`
 * takes either a directory of jars or a ready-made colon-separated classpath,
 * which is what a Gradle build hands you.
 */
const KT_OUT = process.env.MCPD_E2E_KOTLIN_PROBE ?? '/tmp/ktprobe'
const KT_SDK = process.env.MCPD_E2E_KOTLIN_SDK ?? '/tmp/ktbuild'
const KT_DEPS = process.env.MCPD_E2E_KOTLIN_DEPS ?? '/tmp/kdeps'

function goBin() {
  if (which('go')) return 'go'
  return existsSync('/tmp/go/bin/go') ? '/tmp/go/bin/go' : undefined
}

function java() {
  if (which('java')) return 'java'
  return existsSync('/tmp/jdk-17.0.2/bin/java') ? '/tmp/jdk-17.0.2/bin/java' : undefined
}

/** The elixir launcher, and the directory its `erl` needs on PATH. */
function elixir() {
  if (which('elixir')) return { run: 'elixir', bin: undefined }
  const local = '/tmp/edl/root/usr/lib/elixir/bin'
  if (existsSync(`${local}/elixir`)) {
    return { run: `${local}/elixir`, bin: '/tmp/edl/root/usr/lib/erlang/bin' }
  }
  return {}
}

/** The kotlinc launcher, from PATH first and the fixed download second. */
function kotlinc() {
  if (which('kotlinc')) return 'kotlinc'
  return existsSync('/tmp/kotlinc/bin/kotlinc') ? '/tmp/kotlinc/bin/kotlinc' : undefined
}

/**
 * The standard library that goes with the compiler in use. It sits beside the
 * launcher in the compiler's own distribution, so it is found from there
 * rather than from one machine's download directory: a probe compiled by one
 * kotlinc and run against another's stdlib is the mismatch that reads as a
 * language bug.
 */
function kotlinStdlib() {
  const launcher = kotlinc()
  if (!launcher) return undefined
  const resolved = spawnSync('sh', ['-c', `command -v ${launcher}`], { encoding: 'utf8' }).stdout.trim()
  const home = path.dirname(path.dirname(realpathSync(resolved || launcher)))
  const jar = path.join(home, 'lib', 'kotlin-stdlib.jar')
  return existsSync(jar) ? jar : undefined
}

/** Every jar the Kotlin SDK needs at runtime, plus the standard library. */
function kotlinClasspath() {
  if (KT_DEPS.includes(':')) return KT_DEPS
  const jars = existsSync(KT_DEPS)
    ? spawnSync('sh', ['-c', `ls ${KT_DEPS}/*.jar`], { encoding: 'utf8' }).stdout.trim().split('\n')
    : []
  return [...jars, kotlinStdlib()].filter((j) => j && existsSync(j)).join(':')
}

/**
 * The runs probes: the same languages, each serving the canonical durable run
 * (tests/e2e/probes/runs/canonical.json) through its own worker. Read by
 * runs.mjs. The curl probe has no entry: the worker protocol is a loop with
 * state, and a shell that drove it would be an SDK by another name.
 */
export const RUN_PROBES = [
  {
    id: 'typescript',
    detect: () => existsSync(path.join(ROOT, 'sdks/typescript/dist/esm/index.js')),
    why: 'sdks/typescript/dist is not built; run `npm ci && npm run build` in sdks/typescript',
    run: (env, opts) =>
      exec('node', ['--experimental-strip-types', 'tests/e2e/probes/runs/probe.ts'], { env, ...opts }),
  },
  {
    id: 'python',
    detect: () => which('python3'),
    why: 'python3 is not on PATH',
    run: (env, opts) =>
      exec('python3', ['tests/e2e/probes/runs/probe.py'], {
        env: { ...env, PYTHONPATH: path.join(ROOT, 'sdks/python') },
        ...opts,
      }),
  },
  {
    id: 'go',
    detect: () => Boolean(goBin()),
    why: 'go is not on PATH',
    run: (env, opts) =>
      exec(goBin(), ['run', '.'], {
        cwd: path.join(PROBES, 'runs/go'),
        env: { ...env, GOFLAGS: '-mod=mod' },
        ...opts,
      }),
  },
  {
    id: 'elixir',
    detect: () =>
      Boolean(elixir().run) && existsSync(path.join(ROOT, 'sdks/elixir/_build/Elixir.Mcpd.StepWorker.beam')),
    why: 'elixir is not on PATH, or sdks/elixir/_build holds no compiled worker (Elixir.Mcpd.StepWorker.beam)',
    run: (env, opts) => {
      const { run, bin: elixirBin } = elixir()
      return exec(run, ['-pa', 'sdks/elixir/_build', 'tests/e2e/probes/runs/probe.exs'], {
        env: { ...env, PATH: elixirBin ? `${elixirBin}:${env.PATH}` : env.PATH },
        ...opts,
      })
    },
  },
  {
    id: 'kotlin',
    detect: () => Boolean(java()) && kotlinProbeClass(path.join(PROBES, 'runs/RunsProbe.kt'), 'RunsProbeKt'),
    why: 'no JDK, or RunsProbe.kt has changed and kotlinc is not available to recompile it',
    run: (env, opts) =>
      exec(java(), ['-cp', `${KT_OUT}:${KT_SDK}:${kotlinClasspath()}`, 'RunsProbeKt'], { env, ...opts }),
  },
  {
    id: 'rust',
    detect: () => which('cargo'),
    why: 'cargo is not on PATH',
    run: (env, opts) =>
      exec('cargo', ['run', '--quiet', '--bin', 'runs'], {
        cwd: path.join(PROBES, 'rust'),
        env,
        timeout: 600_000,
        ...opts,
      }),
  },
]

/** The compiled probe, rebuilt when the source is newer than the bytecode. */
function kotlinProbeClass(source = path.join(PROBES, 'Probe.kt'), className = 'ProbeKt') {
  const compiled = path.join(KT_OUT, `${className}.class`)
  const fresh = existsSync(compiled) && statSync(compiled).mtimeMs >= statSync(source).mtimeMs
  if (fresh) return true
  const compiler = kotlinc()
  if (!compiler || !existsSync(KT_SDK)) return false
  // The SDK is built for JVM 17 (its Gradle toolchain), and an inline
  // function from a 17 build cannot be inlined into 1.8 bytecode, which is
  // what kotlinc emits with no target named. The probe is built for the same
  // target as the SDK it calls.
  // JAVA_HOME is only ever set to a directory that exists. The kotlinc
  // launcher runs `$JAVA_HOME/bin/java` when the variable is set, so naming
  // the fixed download on a machine that has a JDK on PATH and no download
  // would make the compiler fail to start and the language read as skipped.
  const javaHome = process.env.JAVA_HOME ?? (existsSync('/tmp/jdk-17.0.2') ? '/tmp/jdk-17.0.2' : undefined)
  const out = spawnSync(
    compiler,
    ['-jvm-target', '17', '-cp', `${KT_SDK}:${kotlinClasspath()}`, '-d', KT_OUT, source],
    {
    cwd: ROOT,
    env: javaHome ? { ...process.env, JAVA_HOME: javaHome } : { ...process.env },
    encoding: 'utf8',
    timeout: 600_000,
    },
  )
  return out.status === 0 && existsSync(compiled)
}

export function which(cmd) {
  return spawnSync('sh', ['-c', `command -v ${cmd}`], { encoding: 'utf8' }).status === 0
}

/**
 * Run a probe without blocking this process.
 *
 * `spawnSync` would be shorter and is wrong here: the recording proxy below
 * lives in this event loop, so a synchronous child would deadlock against the
 * probe waiting for a reply the harness cannot send until the probe exits.
 */
export function exec(cmd, args, { cwd = ROOT, env, timeout = 120_000 } = {}) {
  return new Promise((resolve) => {
    // Its own process group, so a timeout can reclaim the whole probe.
    // `probe.sh` is a shell that runs curl; killing the shell alone leaves curl
    // holding the inherited stdout pipe, so `close` never fires and the harness
    // waits for ever on a probe it already gave up on. That hung this suite
    // against a server that never answers, which is exactly the case it exists
    // to test.
    const child = spawn(cmd, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], detached: true })
    let stdout = ''
    let stderr = ''
    let settled = false
    const done = (result) => {
      if (settled) return
      settled = true
      resolve(result)
    }
    const timer = setTimeout(() => {
      stderr += `\ntimed out after ${timeout} ms`
      try {
        process.kill(-child.pid, 'SIGKILL')
      } catch {
        child.kill('SIGKILL')
      }
      // Resolved here rather than waiting for `close`: an orphan that survived
      // the kill would hold the pipe open and this promise with it.
      done({ status: 124, stdout, stderr })
    }, timeout)
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (d) => { stdout += d })
    child.stderr.on('data', (d) => { stderr += d })
    child.on('error', (err) => {
      clearTimeout(timer)
      done({ status: 127, stdout, stderr: `${stderr}${err}` })
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      done({ status: code ?? 1, stdout, stderr })
    })
  })
}

/**
 * A comma-separated allowlist of languages, for narrowing a run while working
 * on a suite itself.
 *
 * A language it excludes is REPORTED as skipped, exactly like a missing
 * toolchain, so `MCPD_REQUIRE_ALL_LANGUAGES=1` still fails on it. A filter that
 * quietly shrank a suite would be a way of making it green by doing less, which
 * is the failure these suites exist to prevent.
 */
export function languageFilter() {
  const only = (process.env.MCPD_E2E_LANGUAGES ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  const unknown = only.filter((id) => !LANGUAGES.some((l) => l.id === id))
  if (unknown.length > 0) {
    console.error(
      `MCPD_E2E_LANGUAGES names ${unknown.join(', ')}, which there is no probe for. `
      + `The suite knows ${LANGUAGES.map((l) => l.id).join(', ')}.`,
    )
    process.exit(2)
  }
  return only
}
