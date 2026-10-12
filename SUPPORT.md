# Support

mcpd is free and open-source software, maintained by Internet Technology
Services LLC. There is no paid tier, no hosted version, and no support contract
to buy. Everything below is what exists.

## Start here

| You want | Go to |
| --- | --- |
| To get mcpd running | [The quickstart](https://mcpd.sh/docs/quickstart) |
| What a field means | [The data model reference](https://mcpd.sh/docs/reference/data-model) |
| A flag, a key, or a storage backend | [Configuration](https://mcpd.sh/docs/configuration) |
| To run it for other people | [Deploy](https://mcpd.sh/deploy) |
| To change the code | [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) |

## Where to ask

**A bug, or a feature request:** open an issue at
<https://github.com/mcpd/mcpd/issues>. Search first, since the answer may
already be there in a closed one.

**A question about using mcpd:** open an issue as well, and say in the title
that it is a question. There is no separate forum, and a question with an answer
attached is more useful to the next person than a private reply.

**A security issue:** never in public. Follow [`SECURITY.md`](SECURITY.md) and
email hi@mcpd.sh.

**Anything about conduct:** [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md), also
hi@mcpd.sh.

**Anything else,** including licensing and the contributor agreement:
hi@mcpd.sh.

## What to put in a bug report

The daemon prints the configuration an invocation resolved to, which answers
most of the questions a maintainer would otherwise have to ask:

```shell
mcpd config
```

Paste that, with any secret removed, along with:

- the version, from `mcpd --version`, or the image tag,
- the storage backend: the embedded file, Postgres, or ClickHouse,
- the SDK and language version, if a signal arrived wrong or not at all,
- what you expected, and what happened instead,
- the smallest way to reproduce it.

Two things narrow a report down fast, and both are quick:

1. **Does it reproduce with demo data?** `mcpd demo --data /tmp/mcpd-bug.db`
   starts a daemon with a sample run in it, on nothing else.
2. **Does it reproduce on the embedded store?** If yes, the storage backend is
   not the cause, and that removes half of the search.

If mcpd rejected something you sent, include the request and the exact error
text. Errors here are written to name the problem and the recovery, so the
message usually carries the answer.

## Deprecation, and the support window

mcpd is before 1.0 and has no tagged release yet, so this is the policy the
first release will be cut under, written down now so nobody has to infer it.

**The support window is the latest release.** Fixes, including security fixes,
land on `main` and ship in the next release. There are no maintained older
branches and no long-term-support line; the remediation for any advisory is
to upgrade. If that ever changes, this file will say which releases are
maintained and for how long.

**A breaking change to the wire contract is announced before it lands.** The
wire contract is `spec/mcpd.spec.json`: the HTTP API and every generated SDK
type, in six languages at once. Removing or renaming an endpoint or a field,
or changing what an existing value means, is a breaking change. Before 1.0 the
minimum notice is one minor release and 30 days, whichever is longer: the
release that announces the change keeps the old behaviour working and marks it
as deprecated in `CHANGELOG.md`, in the reference documentation and, where the
SDKs can, in a deprecation warning, and a later release removes it. Adding a
field, an endpoint or an enum value is not a breaking change and carries no
notice. The daemon's flags follow the same rule; an old flag keeps working for
the notice period and says what replaced it.

**After 1.0**, a breaking wire-contract change is a major version, and the
notice becomes one minor release and 90 days.

## What is not supported

- **Older releases.** mcpd is before 1.0. Fixes land on `main` and in the next
  release, and there are no maintained older branches. See the support window
  above.
- **Private debugging of your deployment.** Ask in the open, where the answer
  helps everyone. We cannot read your logs for you.

## Response times

These are aims, not commitments. There is no rota.

| Kind | Aim |
| --- | --- |
| Security report | Acknowledged within 3 working days |
| Issue | Triaged within 10 working days |
| Pull request | A first response within 5 working days |

If a thread has gone quiet past that, post a reminder on it. That is welcome,
not a nuisance.

## Escalation

There is no separate escalation path, no phone number and nobody on call. The
escalation contact is the maintainers, at **hi@mcpd.sh**, and the aims above
are the response times that mailbox works to: a security report is acknowledged
within 3 working days, an issue is triaged within 10, a pull request gets a
first response within 5. Put "escalation" and the issue number in the subject
so it is not read as a duplicate. If you are assessing mcpd for an estate that
needs more than an aim, that is the honest answer, and the alternatives are a
second organisation with commit rights or an escrow arrangement, which is a
conversation to have at the same address.
