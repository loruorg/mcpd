# Maintainers

mcpd is maintained by **Internet Technology Services LLC**, which holds the
copyright in the project. The code is free and open-source software under
[Apache-2.0](LICENSE), and it stays that way.

Reach the maintainers at **hi@mcpd.sh** for anything: a licensing question, a
conduct report, a security issue, or a change too large to open cold as a pull
request. Security reports follow [`SECURITY.md`](SECURITY.md), not a public
issue.

Individual maintainers are listed as the GitHub team `@mcpd/maintainers`, which
is what [`.github/CODEOWNERS`](.github/CODEOWNERS) resolves to. That team is the
current list. This file describes what the role covers, so nobody has to guess
who decides what.

## Who reviews what

Every area below has the same owners today. They are named separately because
they carry different risk, and a review of one is not a review of another.

| Area | Paths | What a review is checking |
| --- | --- | --- |
| The specification | `spec/`, `tools/codegen/` | A change here is reproduced into every language and every generated document at once. Field names and defaults are close to permanent. |
| The daemon | `crates/` | Correctness, and that no request from the network can panic the process or grow it without bound. |
| Storage | `crates/mcpd-store/`, the schema files | Every backend, not the one the author tested. A migration, never an edit to a released schema. |
| The SDK contract | `sdks/SDK-CONTRACT.md` | Behaviours B1 to B10 hold in every language, or none. |
| The SDKs | `sdks/`, `crates/mcpd-sdk/` | Conformance with the contract, and that instrumentation cannot break the agent it observes. |
| The web surfaces | `web/` | `DESIGN.md`, the design tokens, and that content is visible without an animation running. |
| Deployment | `deploy/`, `Dockerfile` | That the manifests match the flags the binary actually has. |
| Governance and CI | `.github/`, this file and the other governance documents | Anything that changes who can merge, or what CI proves. |

## What a maintainer does

- Reviews pull requests, and says what would be accepted when rejecting one.
- Keeps `spec/mcpd.spec.json` and `sdks/SDK-CONTRACT.md` coherent, since both
  are propagated mechanically and a mistake in either is expensive to undo.
- Triages issues, and closes the ones that are answered.
- Cuts releases and writes what changed, under the deprecation policy and
  support window in [`SUPPORT.md`](SUPPORT.md).
- Handles security reports on the timeline in [`SECURITY.md`](SECURITY.md).
- Handles conduct reports under [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md). A
  maintainer takes no part in handling a report about themselves.

## How decisions are made

Technical decisions are made in the open, in the issue or the pull request, and
the reasoning is written down where the next reader will find it. When
maintainers disagree, the one who owns the area decides, and records why.

Three rules constrain every decision, including a maintainer's own:

1. **The spec comes first.** A change to the data model or the HTTP API starts
   in `spec/mcpd.spec.json`, never in a generated file.
2. **The contract comes before the code.** A change to how an SDK behaves starts
   in `sdks/SDK-CONTRACT.md`, and then lands in every language.
3. **`make check` is not weakened to land a change.** If a check is wrong, the
   check is fixed on its own terms, in its own commit.

Anything that changes the licence, the outbound licensing of contributions, or
the terms of the contributor agreement is decided by Internet Technology Services
LLC, since those are the copyright holder's to decide.

## Response times

These are what we aim for, not a contractual commitment. mcpd has no paid
support tier funding a rota.

| Kind | Aim |
| --- | --- |
| Security report | Acknowledged within 3 working days |
| Conduct report | Acknowledged within 3 working days |
| Pull request | A first response within 5 working days |
| Issue | Triaged within 10 working days |

If something has gone past these and you have heard nothing, say so on the
thread, or email hi@mcpd.sh. Mail does get lost, and a reminder is welcome.
That address is also the escalation contact, and the aims in this table are
what it works to; there is no separate path with a firmer number behind it.

## Cutting a release

1. Set `version` under `[workspace.package]` in `Cargo.toml`, and the same
   version everywhere `make check` names: the chart, the manifests, the
   compose file, every SDK manifest and user agent. `make check` fails until
   they all agree.
2. Move `Unreleased` in `CHANGELOG.md` under the new version, merge, and wait
   for `ci.yml` to pass on `main`.
3. Tag that commit `vX.Y.Z` and push the tag. `release.yml` does the rest: the
   archives and `SHA256SUMS`, the image for `linux/amd64` and `linux/arm64`
   with an SBOM, the Helm chart at `oci://ghcr.io/mcpd/charts/mcpd`, the Go
   module tag `sdks/go/vX.Y.Z`, provenance for each, and the GitHub release.
4. On the first release only, the `public` job fails, because GitHub makes a
   new package private. Open the package settings for the image and for
   `charts/mcpd`, set each to public, link each to this repository, and re-run
   the failed jobs.

Nothing publishes the npm, PyPI, crates.io, Maven Central or Hex packages yet.
Each needs an account and a token in this repository's secrets, and neither
exists. The Kotlin build signs only when a key is given as the `signingKey`
Gradle property, so it is ready for that step and does not require it.

## Becoming a maintainer

There is no application. Maintainers are invited, and the route is the obvious
one: land good changes, review other people's, and answer issues you know the
answer to. What is being looked for is judgement, over volume.

An invitation names the area you will own, from the table above. Ownership can
be narrow. Someone who maintains the Kotlin SDK and nothing else is a maintainer.

## Standing down

Tell the other maintainers, in the open where you can. You keep the credit for
what you contributed. Access is removed the same week, which is a security
practice and not a judgement on anyone.

A maintainer who has been unreachable for six months may be moved out of the
team by the remaining maintainers, and is welcome back on request.
