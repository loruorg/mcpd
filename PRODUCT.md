# Product

<!-- impeccable:product-schema 1 -->

> **Provenance note.** This record was written without a live interview. The user
> gave an explicit brief and twice instructed that no questions be asked
> ("Do not ask me any questions because I'm asleep"; "make good judgements
> without asking me questions"). Facts taken directly from that brief are marked
> **[brief]**. Facts inferred by the author are marked **[inferred]** and are
> open to correction. Nothing here is invented evidence.

## Platform

web

## Stack

**[brief]** Fixed by the user:

- Backend: **Rust**.
- Web surfaces (marketing site, documentation, and the product GUI): **Astro +
  shadcn + Tailwind CSS**.
- Client SDKs: **TypeScript/JavaScript, Python, Rust, Go, Kotlin**.
- Deployment targets: **Docker and Kubernetes**, both first-class.

**[inferred]** Supporting choices the brief delegates by omission: a single
statically-linked Rust binary as the default distribution; embedded storage for
the zero-dependency path and ClickHouse/Postgres for scale; OTLP as the wire
protocol so the tool is framework-agnostic rather than SDK-locked.

## Users

**[inferred, from the brief's "both solo operators and enterprise"]**

**Primary — the solo operator.** One engineer shipping an LLM agent, usually
alone or on a team of two or three. Their situation: the agent works on their
machine and misbehaves in production, and they cannot see why. They are on a
laptop or a single small VPS. Their job is to find the one bad run out of
thousands, read what the model actually received and returned, and know what it
cost. They will abandon any tool that needs a database cluster, a signup, or an
afternoon of YAML before it shows them a trace.

**Secondary — the platform team at an enterprise.** They run agents for other
teams inside the company. Their situation: several product teams, data that
cannot leave the network, an auditor who asks who saw what. Their job is to give
every team the solo operator's experience while enforcing retention, redaction,
access control, and a cost ceiling — on their own Kubernetes cluster.

Both audiences use the same product. The enterprise features are additive
configuration, never a separate build.

## Product Purpose

**[brief]** A free and open-source tool for **agent observability** that can be
deployed quickly, integrated into an existing project easily, and used without
adopting a framework.

**[inferred]** What it does concretely: it receives traces, metrics, logs,
scores, and human feedback from AI agents; stores them; and presents them as
runs you can read — a waterfall of every agent step, model call, tool call, and
retrieval, with the exact prompt, the exact completion, token usage, latency,
and cost attached to each.

Success means an engineer goes from `docker run` to a readable trace of their
own agent in under five minutes, and an enterprise runs the identical binary on
Kubernetes under their own auth and retention rules with no vendor account.

## Positioning

**[inferred from the brief's "alternative for ../mastra"]**

The incumbent (`../mastra`) is a **TypeScript agent framework** that includes
observability as one subsystem. Its observability is excellent but arrives
attached to the framework: to be seen, an agent is expected to be built with it,
in TypeScript.

`mcpd` inverts that. It is **not a framework** — it is a daemon. The claim a
neighbouring product cannot truthfully copy:

> Observability that is a *process*, not a dependency. One static Rust binary,
> no runtime, no required database, speaking OTLP — so it observes agents
> written in any language, with any framework, including ones that already
> exist.

This is the `statsd` / `collectd` / `fluentd` lineage applied to AI agents, and
it is the reason the name ends in `d`. The consequences that follow from it are
the positioning: a solo operator can run it beside their app with no
infrastructure; an enterprise can run the same binary as a StatefulSet; and
neither has to rewrite their agent to be observed.

## Operating Context

**[inferred]**

- Agents run in Node, Python, Rust, Go, and JVM processes, in serverless
  functions, in long-lived workers, and in CI.
- The observability signal is bursty and lopsided: long idle periods, then a
  storm of nested spans with very large text payloads (prompts and completions
  are kilobytes, not bytes).
- Investigation is almost always retrospective and starts from a complaint: a
  bad answer, a runaway bill, a timeout. The entry point is search, not a
  dashboard.
- The comparison is done in a terminal and a browser side by side. Copying a
  working snippet into an existing codebase is the moment of adoption.
- Enterprise deployment happens through Helm or a Kubernetes operator, behind an
  existing identity provider, with data kept in the customer's own store.

## Capabilities and Constraints

**[brief] Required:**

- Full feature parity with `../mastra`'s observability surface.
- More value than the incumbent for solo operators *and* for enterprise.
- Runs well in Kubernetes and in Docker.
- Free and open source.
- SDKs for TypeScript/JavaScript, Python, Rust, Go, and Kotlin.
- Documentation showing code examples with correct syntax highlighting and an
  easy way to switch between languages.
- Site and docs SEO-optimised and shareable.
- Every page animated, each carrying at least one motion graphic.

**[inferred] Technical constraints that follow:**

- The wire protocol must be open (OTLP/HTTP + a native JSON ingest) so that
  parity does not require our SDK.
- Prompt and completion payloads must be redactable *before* storage, since the
  enterprise audience cannot store raw PII.
- The zero-dependency path and the horizontally-scaled path must be the same
  binary with different configuration.
- The GUI is Astro-based, so trace exploration must work as islands over a real
  HTTP API rather than assuming a single-page React app.

**Terminology** (to be used consistently in product, SDKs, and docs):
*trace* (one end-to-end agent run) · *span* (one step inside it) · *run* ·
*score* (machine evaluation) · *feedback* (human evaluation) · *session* ·
*project* · *environment*.

**Decided since**: the licence. Apache-2.0, held by Internet Technology
Services LLC, over the whole tree: the daemon, all six SDKs, the interface, the
site and the specification. There is no second licence and no directory held
back from it, which `tools/check-consistency.mjs` now refuses mechanically
rather than trusting to memory.

**Explicitly undecided** — recorded rather than invented: hosted/cloud offering
(none is claimed); pricing (the product is free; no paid tier is claimed
anywhere).

## Brand Commitments

**[brief]**

- Name: `mcpd` (the repository directory the user created). **[inferred]** Read
  as a daemon name, lowercase, always set in the product's mono voice.
- Visual direction: "vercel-like styles (not exactly)" — precise, technical,
  high-contrast, generous space. Explicitly *not* a copy.
- Theme: **light**. A light theme is the committed default.
- Motion is a brand commitment, not decoration: every page animated, every page
  carrying at least one motion graphic.
- The brief bans slop explicitly, and the project's design law is binding.

## Evidence on Hand

- `../mastra` is present on disk and is real, readable source. It is the
  parity reference and is being audited directly, not from memory.
- **No customers, no testimonials, no benchmarks, no press, no logos, and no
  adoption numbers exist.** Future work must not fabricate any of these. Social
  proof, if ever shown, must come from something real (e.g. the project's own
  GitHub activity) or be omitted entirely.
- No brand assets exist yet: no logo, no typeface licence, no photography. Any
  mark must be authored as part of the work.

## Product Principles

**[inferred]** Derived from the confirmed brief:

1. **A process, not a dependency.** Anything that forces a user to restructure
   their agent to be observed is a design failure. Meet code where it already
   lives.
2. **Five minutes to the first trace.** The default path has zero required
   configuration and zero external services. Every feature must survive the
   question "does this add a step before the first trace?".
3. **One binary, two audiences.** Enterprise capability is configuration on the
   same artifact the solo operator runs — never a fork, a licence key, or a
   feature gate.
4. **Show the payload.** The prompt and the completion are the product. Anything
   that summarises them away is hiding the evidence the user came for.
5. **Open at every seam.** Open protocol in, open format out, open licence
   around it. The user must be able to leave.

## Accessibility & Inclusion

**[inferred]** No user-specific requirement was established. The working default
is WCAG 2.2 AA: real contrast on a light theme, full keyboard operation of the
trace explorer, visible focus, and every motion commitment gated behind
`prefers-reduced-motion` — a page that never animates must still be complete and
readable.
