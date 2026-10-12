# `spec/` — the single source of truth

Everything in this repository that describes **what mcpd's data looks like** or
**what its HTTP API accepts and returns** is generated from `mcpd.spec.json`.
Nothing here is documentation *about* the code; the code is derived from this.

## What is generated from this file

| Target | Path | Generator |
| --- | --- | --- |
| Rust model types | `crates/mcpd-core/src/generated.rs` | `tools/codegen` |
| TypeScript types | `sdks/typescript/src/generated.ts` | `tools/codegen` |
| Python types | `sdks/python/mcpd/_generated.py` | `tools/codegen` |
| Go types | `sdks/go/generated.go` | `tools/codegen` |
| Kotlin types | `sdks/kotlin/src/main/kotlin/dev/mcpd/Generated.kt` | `tools/codegen` |
| Elixir types | `sdks/elixir/lib/mcpd/generated.ex` | `tools/codegen` |
| OpenAPI 3.1 document | `spec/openapi.json` | `tools/codegen` |
| Docs API reference | `web/src/content/reference/api-*.md` | `tools/codegen` |
| Docs data-model reference | `web/src/content/reference/data-model.md` | `tools/codegen` |

## The rule

> Change the spec, run `make gen`, commit the result.

`make check` fails when any generated file differs from what the spec produces,
so drift cannot reach `main`. See [`AGENTS.md`](../AGENTS.md) for the full
contract that agents and humans both follow.

## Layout of `mcpd.spec.json`

- `enums` — closed value sets (span kinds, statuses, signal types).
- `types` — reusable object shapes referenced by entities and endpoints.
- `entities` — the persisted domain objects.
- `endpoints` — the HTTP surface, grouped by tag.

Every field carries a `doc` string. Those strings are the *only* source of the
prose in the generated reference documentation, so write them for a reader.

## `definition.schema.json`

The body of a workflow definition, the document a worker registers and
`POST /v1/api/definitions` stores, is not described by `mcpd.spec.json`: it is
a JSON value inside a `DefinitionDocument`, with a grammar of its own. That
grammar is `definition.schema.json`, a JSON Schema 2020-12 document written by
hand to section 3 of `internal/run-contract-v1.md`. It is published beside
`openapi.json`, so an editor, a CI job or a builder in any language can check a
document before posting it.

The daemon checks the same rules in Rust, in `crates/mcpd-runs`. The schema
refuses a shape the walker could not run; the Rust validator refuses that and
what a schema cannot express, such as an id used twice, a path that reads
`item` outside a `foreach` body, or a `matches` pattern that does not compile.
A body that fails either is a 400 naming the field.

The schema carries one example document per node kind and one complete
example under `examples`. The same documents sit under
`crates/mcpd-runs/tests/documents/valid/`, and the documents the validator must
refuse sit under `invalid/`, each beside a `.reason` file that says why. A
reason that starts with `rust-only:` names a rule the schema cannot express.

This file is not generated and `make gen` does not touch it. Change it by
hand, and change the Rust validator with it.
