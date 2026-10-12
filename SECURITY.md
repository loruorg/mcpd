# Security policy

Report a suspected vulnerability to **hi@mcpd.sh**, with "Security" in the
subject line. Do not open a public issue, discussion or pull request for it.

We use coordinated disclosure: you tell us privately, we fix it, and we publish
together.

## Why this project is worth your attention

Two facts about mcpd shape its threat model, and both are on purpose.

**It stores prompts and completions.** That is the most sensitive data most
users have. A trace holds the exact text sent to a model and the exact text that
came back, along with the identifiers that tie a run to a session, a user and a
service. A read primitive in mcpd is not a leak of metrics. It is a leak of
everything the agent was told and everything it said.

Redaction is the control that stands in front of this. It runs before anything
is written, across all six signals, so a value a rule matches never reaches the
disk. A way to get a value past a configured redaction rule and into storage is
a vulnerability, and we want to hear about it.

**Ingest is open by default.** With no keys configured, every scope is open,
because the first run on a laptop must not require a credential. The default
listen address is `0.0.0.0:4317`. When the daemon binds a non-loopback address,
it logs a warning at startup for each surface open to a caller that should not
reach it, naming the surface and the flags that close it. That default is
documented, so an unauthenticated instance someone chose to expose is not in
itself a report we can act on. Anything that gets past a key that **was** configured is, and so is
anything that lets an ingest key read.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Supported |

0.1.0 is the first release, so there is no older line to support.

## In scope

- **The daemon.** Everything under `crates/`: ingest, the query API, the
  redaction pipeline, sampling, API-key authentication and its scopes, the OTLP
  decoders, the storage backends, and the operator interface the binary serves.
- **The SDKs.** `sdks/` and `crates/mcpd-sdk`, including anything that could
  make an SDK leak credentials, corrupt another process's data, or break the
  application it is instrumenting.
- **The deployment manifests.** `deploy/`, meaning the Compose file, the
  Kubernetes manifests and the Helm chart, plus the `Dockerfile`.
- **The specification and the generator.** `spec/` and `tools/codegen/`, where a
  flaw is reproduced into every language at once.

Reports we particularly want:

- A read of any signal without a valid read key, or with an ingest key.
- Any bypass of a configured redaction rule.
- Cross-project reads: data from one project reachable through another.
- Injection into a storage backend through a span, log, metric label, score or
  feedback field.
- Remote crash, panic or unbounded memory growth from a single request.
- Anything that lets a recorded value execute in the operator interface.

## Out of scope

- **The marketing site's content.** The pages and copy published at mcpd.sh
  are documentation. A wrong sentence there is a documentation bug: open a
  normal issue.
- Missing hardening headers or a missing SPF record on the static site, with no
  demonstrated impact.
- Findings from an automated scanner with no working case behind them.
- Volumetric denial of service, meaning flooding an endpoint with otherwise
  valid traffic. A single request that costs a disproportionate amount of CPU or
  memory is in scope, and is a different thing.
- The documented open defaults described above, when no key was configured.
- Attacks that need physical access to the host, a compromised operating system
  account, or social engineering of a maintainer.
- Vulnerabilities in a dependency with no path to exploiting them through mcpd.
  Report those upstream, and tell us if we should pin or patch.

## What to include

The faster we can reproduce it, the faster it is fixed.

- The type of issue and the impact you think it has.
- The version or commit, and the storage backend, if it matters.
- The exact paths in the source, and the configuration needed to reproduce.
- Steps, a request, or a proof of concept.
- Anything you know about who is affected.

Test against your own instance. Never test against anyone else's data.

## What to expect

- **Acknowledgement within 3 working days.** If you have not heard back, send it
  again: mail does get lost.
- **An assessment within 10 working days**, saying whether we can reproduce it,
  what severity we think it has, and what we plan to do.
- **A fix aimed at 30 days for a critical issue**, longer for one that needs a
  change to the specification or a storage schema, and we will tell you if that
  is the case rather than going quiet.
- **Disclosure coordinated with you.** We will agree a date, publish an advisory
  on the repository, and credit you by whatever name you ask for, or not at all
  if you prefer.

mcpd is before 1.0. Fixes land on `main` and in the next release. There are no
maintained older branches, so upgrading is the route to a fix.

There is no bug bounty. There is no paid tier funding one. We can offer credit,
a clear advisory and a fast fix.

## AI-assisted reports

Use whatever finds bugs, including a model reading the code, a fuzzer, or static
analysis. Some of it works well.

The report has to be written and verified by a person. Before you send it,
confirm the issue exists in the current code, at the paths you cite, and that you
can reproduce it. An unverified model-written report is not a security report,
and it will be closed without analysis.

Say whether AI was involved and what it did. That disclosure is never held
against you. Leaving it out is what ends the conversation.

## Safe harbour

If you follow this policy, act in good faith, stay within scope, and avoid
accessing or destroying anyone else's data, we will treat your research as
authorised and will not pursue or support legal action against you for it. If a
third party takes action over research that followed this policy, we will say so
on your behalf.
