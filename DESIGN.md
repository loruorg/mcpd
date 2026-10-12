# Design

<!-- impeccable:design-schema 1 -->

The visual system for `mcpd`, documented from the built site rather than from
intention. Every value here is the one in `web/src/styles/panel.css`.

The direction contract this was built to lives as an HTML comment at the top of
`<body>` in `web/src/layouts/Base.astro` (seed `afd87fbc`), and survives into the
production build.

---

## The idea

**The page is a painted instrument panel.** Every decision below follows from
that one, and the reason it is that and not something else is the product: mcpd
is a daemon you run next to your agent, so the site is an instrument already
reading a signal rather than a brochure describing one.

It deliberately refuses two things: the dark neon developer-tool landing page
that this category always ships, and its predictable opposite, the white Swiss
SaaS page with a soft product screenshot and a gradient.

## Surfaces

Depth is **tonal, never a shadow**. A raised surface is a few percent lighter
than the one beneath it; its edge is a hairline in its own colour with a light
lip along the top, the way light catches a rolled edge on painted metal. There
is not one drop shadow in the system.

| Token | Value | What it is |
| --- | --- | --- |
| `--color-panel-0` | `#d4d5ca` | The ground: the page itself |
| `--color-panel-1` | `#dedfd5` | A panel laid on the ground |
| `--color-panel-2` | `#e8e9e0` | The top surface, catching light |
| `--color-panel-3` | `#f1f2ea` | The brightest lip |
| `--color-well` | `#c6c8bb` | A recess: inputs, code, sunken fields |
| `--color-chart` | `#edeee5` | Chart paper, set into the panel |
| `--color-floor` | `#21231d` | The darker floor the page closes on |
| `--color-floor-ink` | `#dcded2` | Ink on that floor |

The palette is a warm mineral grey-green, the colour instrument panels are
actually painted. It is deliberately **not** the cream that AI-made pages
default to, **not** the cool `gray-100` that ships as a UI kit's neutral, and
**not** the blue-charcoal that dark developer tools default to.

Classes: `.panel`, `.panel-raised` (lip on top, raised) and `.well` (lip on the
**bottom**, because a sunken face catches light on its lower inside wall;
getting that backwards is what makes fake depth look fake).

## Ink

| Token | Value | Use |
| --- | --- | --- |
| `--color-ink` | `#191a15` | Primary text |
| `--color-ink-2` | `#43453b` | Body and secondary text |
| `--color-ink-3` | `#53544a` | Legends, labels, tertiary text |
| `--color-ink-4` | `#8d8f80` | **Rules and ticks only. Never text.** |

Warm near-black with a green cast, never blue-black.

Three text levels, and **every one clears 4.5:1 on every surface in the system,
including the recessed well** (measured: ink 10.3-15.5, ink-2 5.75-8.65, ink-3
4.53-6.81). A fourth grey light enough to read as a further step cannot also
clear that bar on a panel this light, so `ink-4` is a rule colour and
`tools/check-consistency.mjs` fails the build if it is ever used for text.

## Signal colour

| Token | Value | Meaning |
| --- | --- | --- |
| `--color-amber` | `#7b4705` | Attention |
| `--color-oxide` | `#93301e` | Fault |
| `--color-olive` | `#3f5b29` | Nominal |
| `--color-slate` | `#3f5560` | Model activity |

Signal colours **mean something**. Amber is not a highlight, it is attention.
Nothing on the site is tinted for decoration. All four are deep and desaturated,
tuned so they clear 4.5:1 on every surface, which also lands them in one tonal
band rather than four unrelated brightnesses.

## Lines

| Token | Value |
| --- | --- |
| `--color-scribe` | `rgba(25, 26, 21, 0.16)` |
| `--color-scribe-2` | `rgba(25, 26, 21, 0.09)` |
| `--color-lip` | `rgba(255, 255, 255, 0.66)` |

`.seam` is the scribed division between panel sections: one hairline with a
light lip beneath it, two sheets of painted metal meeting.

## Type

| Role | Face | Source |
| --- | --- | --- |
| Display and panel legends | **Technor** (variable 200-900) | Fontshare, self-hosted |
| Body and interface | **Supreme** (variable 100-800) | Fontshare, self-hosted |
| Data, code, measurement | **Tabular** (variable 300-700) | Fontshare, self-hosted |

Chosen after rendering candidates side by side, not from reputation. Technor is
a squarish aerospace-instrument grotesque in the Eurostile lineage, which is
what real console legends are cut in. Tabular is a technical mono, used only for
genuine data, code and measurement, never as a costume for "technical".

None is on the design law's rejected list, and none is a Google Fonts default.
All three are self-hosted woff2 in `web/public/fonts/`, preloaded.

`.display` sets tracking to `-0.022em` with `text-wrap: balance`. `.legend` is
Technor 600 at `0.6875rem` with `0.18em` tracking, uppercase.

## The label rule

`.legend` labels **a region of the panel**. It is not an eyebrow above a
heading, it is never used as one, and it is not the treatment for navigation,
buttons or tabs, which each have their own. This is deliberate: one small-text
costume worn by every role at once is a template, not a voice.

## Radius

`--radius-panel: 2px`, `--radius-tile: 1px`. Almost square. A panel is cut, not
moulded.

## Components

- **`.annunciator`** — a rectangular tile with engraved lettering that **latches**
  on real state, the way a console annunciator lights on a fault and stays lit.
  Unlit is the panel's own recessed tone; lit is its signal colour. Used only for
  genuine system state, never as a metadata chip.
- **`.ticks` / `.ticks-major`** — a calibration scale. It appears only where
  something is actually being measured, never as a background grid.
- **`.field`** — selects and inputs with `appearance: none` and a chevron drawn
  in the house stroke, so no OS widget ever appears on the panel.

## Texture

A single SVG `feTurbulence` fractal noise at `0.035` opacity across the page.
Felt, not seen: it kills gradient banding and stops large flat areas reading as
flat vector fills. Deliberately not a repeating rule, which is ruled paper and
aliases into scanlines at 1x device pixel ratio.

## Motion

Every page carries at least one motion graphic, enforced by
`tools/check-consistency.mjs`, which fails the build on a page that imports one
without rendering it.

**Content is visible by default.** Nothing on this site starts at `opacity: 0`
waiting for a script. Every animation is on something already painted, and every
one has a `prefers-reduced-motion` branch that leaves the content complete.

The graphics, all authored for this product:

- **StripChart** — the signature. A chart recorder: ruled paper feeds past a
  fixed pen while the nib rides the ink. The waveform is a sum of sines whose
  periods all divide the run width, so the loop closes with no join; the nib's
  240 keyframes are computed from the same array the path is drawn from, so it
  cannot drift off the line.
- **Waterfall** — a real span tree with bars positioned from actual timings, on
  one coordinate system with its own ruler, swept by a playhead.
- **SignalFlow** — five signal paths converging on one `trace_id`.
- **PipelineDiagram** — a carrier travelling the ingest pipeline, lighting each
  stage as it reaches it.
- **TerminalCast** — a recorded session revealed line by line; the transcript is
  in the markup and fully opaque from the first paint.
- **SearchDemo** — cycling queries; the first is in normal flow and opaque, so a
  dead animation engine leaves one populated search field, not four stacked.

## Layout

Content sits in a `max-w-[86rem]` measure. Prose is held to 68ch; tables and
diagrams are exempt, because data does not fit a reading measure. Docs use a
three-column grid at `xl`: index, article, and an "on this page" rail built from
the rendered headings.

More space above a heading than below it, everywhere.

## What this system refuses

No kicker or eyebrow above a heading. No icon in a tinted tile. No pill badges
for metadata. No gradient anywhere, in text or background. No glow. No card
triad as page structure. No filled-plus-outlined button pair. No drop shadows.
No fake window chrome. No hover lift. No emoji or icon-pack glyphs. No hard-coded
colour outside the token block.

Several of these are enforced mechanically by `tools/check-consistency.mjs` and
run in CI, not left to discipline.
