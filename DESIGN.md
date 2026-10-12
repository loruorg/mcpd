# Design

<!-- impeccable:design-schema 1 -->

The visual system for `mcpd`. Every value here is the one in
`web/src/styles/panel.css`. The direction contract lives as an HTML comment at
the top of `<body>` in `web/src/layouts/Base.astro`.

---

## The idea

**The unopinionated agent observatory.** The site shows the product working:
spans arriving from many frameworks into one waterfall, a cost meter adding
up, a workflow surviving a crash. It does not draw metaphors for the product.

The look is calm, technical and quiet: neutral surfaces with a faint green
cast, hairline edges, one brand green, and signal colours that each name a
kind of thing. It refuses the neon developer-tool page and the gradient SaaS
page alike.

## Two themes, one token set

Every colour is written once, as a `light-dark()` pair, so the light and the
dark value of a token sit on one line and cannot drift.

- `:root { color-scheme: light dark }` follows the reader's system, with no
  script.
- The theme control in the rail writes `data-theme="light|dark"` on `<html>`
  and stores the choice in `localStorage` under `mcpd:theme`.
- An inline script in the `<head>` restores a stored choice before the first
  paint, so there is no flash.
- During a switch, `.theme-switching` disables transitions for two frames so
  the page changes in one step.

Code blocks follow the theme the same way: Shiki writes `--shiki-light` and
`--shiki-dark` into every token (`defaultColor: false`) and `panel.css` picks
one with `light-dark()`. Both syntax themes are in `web/src/lib/shiki-theme.ts`.

## Surfaces

The token names are older than this palette and are kept, because the
operator interface and every page read them.

| Token | Light | Dark | What it is |
| --- | --- | --- | --- |
| `--color-panel-0` | `#f6f7f5` | `#0b0d0c` | The page |
| `--color-panel-1` | `#eff1ee` | `#101312` | A band, a sidebar, a figure header |
| `--color-panel-2` | `#ffffff` | `#141816` | A card |
| `--color-panel-3` | `#f9faf8` | `#1a1f1c` | Hover, the selected row |
| `--color-well` | `#f0f2ef` | `#0d100f` | Sunken: code, inputs, tracks |
| `--color-chart` | `#fbfcfa` | `#111413` | Ground under a chart |
| `--color-floor` | `#0e1211` | `#070908` | The inverse band that closes a page |

Depth: in the light theme a card has a hairline edge and a very soft shadow
(`--shadow-card`); in the dark theme it is a lighter surface and an edge,
because a shadow on near-black reads as dirt. Floating elements use
`--shadow-float`; overlays use `--scrim`.

## Ink

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--color-ink` | `#0f1412` | `#eceeed` | Primary text |
| `--color-ink-2` | `#3a433f` | `#b6bdb9` | Body and secondary text |
| `--color-ink-3` | `#59625d` | `#8d9691` | Labels, legends, tertiary text |
| `--color-ink-4` | `#a7aeaa` | `#47504b` | **Rules and ticks only. Never text.** |

Each text level clears 4.5:1 on every surface of its own theme.
`tools/check-consistency.mjs` fails the build if `ink-4` is used for text.

## Colour that means something

| Token | Light | Dark | Meaning |
| --- | --- | --- | --- |
| `--color-accent` | `#0b7a53` | `#45d394` | Brand: primary action, focus, live |
| `--color-slate` | `#2d5ec2` | `#84a9ff` | A model call |
| `--color-amber` | `#9a5800` | `#f0b44c` | A tool call, or attention |
| `--color-violet` | `#6a4ed6` | `#b1a2ff` | A retrieval or memory read |
| `--color-oxide` | `#c0362c` | `#ff7366` | A failure |
| `--color-olive` | `#1f7a43` | `#62d38f` | Healthy, completed |

`[data-kind="model|tool|retrieval|agent|..."]` sets `--kind` to the matching
colour, so every figure and the interface colour a span the same way. Nothing
is tinted for decoration. Status chips (`.annunciator` with `data-lit`) mix
the signal into the card for their tint.

## Type

| Role | Face |
| --- | --- |
| Display, body and interface | **Supreme** (variable 100-800), self-hosted |
| Data, code, identifiers, labels | **Tabular** (variable 300-700), self-hosted |

Display is Supreme 600 at `-0.032em` tracking with `text-wrap: balance`.
`.legend` is Tabular 500 at `0.6875rem`, uppercase, `0.08em` tracking: it
labels a region (a figure header, a column, a sidebar group), never sits above
a heading as an eyebrow. Chinese, Japanese and Korean pages keep the Latin
faces in front and hand their own script to the system's best face, with
tracking, leading and line breaking set per writing system.

## Radius and lines

`--radius-panel: 8px` for cards and figures, `--radius-tile: 5px` for
buttons, chips and inputs. Lines are `--color-scribe` (edges) and
`--color-scribe-2` (divisions inside a card).

## Components

- **`.btn-primary` / `.btn-secondary`**: solid ink (turning brand green on
  hover) and an outline. At most one pair per view. `.btn-floor` and
  `.btn-floor-secondary` are the same pair on the inverse band.
- **`.fig`, `.fig-head`, `.fig-foot`**: the frame every motion graphic sits in.
  A header strip with a `.legend`, the drawing, an optional caption strip.
- **`.wordmarks`**: integration names set as text. Never other people's logos.
- **`.annunciator`**: a status chip for real state only (firing, failed, up).
- **`.field`**: inputs and selects with no OS widget.
- **`.ticks`**: a scale, only where something is measured.

## Motion

Every page carries at least one motion graphic from the `MOTION` list in
`tools/check-consistency.mjs`. Each explains one product idea with a stylised,
live piece of real UI:

| Graphic | Idea |
| --- | --- |
| `LiveTrace` | One trace fills in live from five frameworks in three languages. |
| `RunViews` | Four views of a run: waterfall, conversation, cost, logs. |
| `Waterfall` | The failing `mcpd demo` run, replayed bar by bar from real timings. |
| `DurableRun` | A worker dies mid-step; the lease expires; another worker finishes it; a wait holds with no process alive. |
| `MonitorChart` | A monitor fires on a threshold, stays firing through silence, resolves on a real reading. The latch is computed by the daemon's rule. |
| `SignalFlow` | Six signals land on one run's timeline, keyed by one trace id. |
| `PipelineDiagram` | A payload rides the ingest pipeline; its credential is redacted before storage, then it is priced and stored. |
| `SamplingGate` | Trace ids hash to a number; every service makes the same keep or drop decision. |
| `SearchDemo` | A query is typed and runs come back with the matching line of the prompt or answer. |
| `LeaseProtocol` | The worker protocol as a sequence diagram, each message sent in turn. |
| `TerminalCast` | A recorded terminal session, revealed line by line. |

Rules, all of them enforced by structure rather than discipline:

- **The markup is the finished state.** A figure that never plays (no
  JavaScript, reduced motion, print) is complete and readable. Playing only
  replays how it got there.
- **One scheduler.** Figures run on `animate()` in `web/src/lib/motion.ts`,
  which plays a figure only while it is on screen and the tab is visible, sets
  `data-playing` (CSS animations inside a figure pause without it), and does
  nothing under `prefers-reduced-motion`.
- **Compositor first.** Bars grow with `transform: scaleX`, playheads move with
  `translateX`, rows appear with `opacity`. No layout is animated in a loop.
- **Computed, not drawn.** A figure derives its frames from the data it
  shows: the waterfall from span timings, the monitor's state from its
  readings, the sampling decision from the hash.
- **No heavy dependencies.** SVG, CSS and a few lines of script.

## Layout

Content sits in a `max-w-[86rem]` measure. Prose is held to 68ch; tables and
figures are exempt. Docs use a three-column grid at `xl`: index, article, and
an "on this page" rail built from the rendered headings. The rail at the top
is translucent over the page and sticky.

## What this system refuses

No gradient text, no glow, no icon in a tinted tile, no pill badges for
metadata, no kicker above a heading, no stock or borrowed logos, no hover
lift, no emoji, no hard-coded colour outside the token block, and no invented
numbers in copy.
