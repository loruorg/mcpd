# mcpd

Read [`AGENTS.md`](AGENTS.md) before changing anything in this repository.

It is the contract for all contributors, human and agent, and it is short. The
part that matters most:

> `spec/mcpd.spec.json` is the single source of truth. The Rust model, all six
> SDK model types, the OpenAPI document and the reference documentation are
> generated from it. Never edit a generated file. Run `make gen`, then
> `make check`.

`AGENTS.md` has a "change → what else must change" table. Find your change in it
and follow the row.
