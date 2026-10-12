## What this changes

<!-- One or two sentences, and the issue it closes: "Fixes #123" or "Refs #123". -->

## Which row of AGENTS.md you followed

`AGENTS.md` has a table of "change → what else must change". Name the row your
change falls under and say what you did for it. If no row fits, say that
instead: a gap in that table is worth knowing about.

<!--
Example:
"You changed the data model or the HTTP API". Added the field to
spec/mcpd.spec.json with a doc string, ran make gen, added the column to
SQLite behind a new migration, then to Postgres and ClickHouse, wired the
handler and its route, and updated the examples the docs read.
-->

## How to check it

<!--
What you ran, with the tail of `make check` pasted in. Name the test that would
have failed before this change: when a bug arrives with no test, the test is
the change. A change to the web surfaces needs a screenshot or a recording.
-->

## Checklist

- [ ] `make check` passes here. It is what CI runs, so a green run here is a
      green run there.
- [ ] No generated file was edited by hand. Anything carrying the
      `GENERATED FILE` banner came out of `make gen`.
- [ ] If the data model or the API changed, `spec/mcpd.spec.json` changed
      first, and every new field has a `doc` string written for a reader.
- [ ] If storage changed, every backend changed: SQLite, Postgres and
      ClickHouse. A released schema gets a migration, never an edit.
- [ ] If an SDK behaviour changed, `sdks/SDK-CONTRACT.md` changed first and
      every SDK now follows it. A behaviour that holds in one language and not
      another teaches the reader something untrue.
- [ ] If a flag or a setting changed, the configuration page, the `mcpd config`
      output and the pre-flight block on the deploy page changed with it.
      `make check` compares them both ways.
- [ ] If a code sample changed, it changed in `examples/`. The documentation
      reads those files and holds no copy of them.
- [ ] Prose follows the house style in `AGENTS.md`: plain and precise, British
      spelling, no em-dashes, no invented numbers.
- [ ] "Allow edits from maintainers" is ticked, and no commit carries a
      `Co-Authored-By:` trailer naming a tool or a "Generated with" line.

## Whether a model helped

Say what a model did, if one was involved, in your own words. Disclosing it is
never held against you, and imperfect English is welcome here. Model English
costs a reviewer time in proportion to how confident it sounds.

<!-- For example: "wrote the first draft of the ClickHouse query". Or: "none". -->

## Your first pull request

Delete this section if this is not your first. If it is, add the line from the
[Contributor License Agreement][cla], with your own name:

> I have read the Contributor License Agreement and I agree to it. Signed,
> `<your full name>` `<your email>`.

Contributions are licensed under Apache-2.0, the licence this repository ships
under. [CONTRIBUTING.md][contributing] has the rest, and `AGENTS.md` is the
contract the code is held to.

[cla]: https://github.com/mcpd/mcpd/blob/main/CONTRIBUTOR_LICENSE_AGREEMENT.md
[contributing]: https://github.com/mcpd/mcpd/blob/main/CONTRIBUTING.md
