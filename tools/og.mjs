#!/usr/bin/env node
/**
 * Share images.
 *
 * A link to this site should look like the site. These are rendered from the
 * same design tokens and the same typeface as the pages themselves, so a link
 * preview is recognisably mcpd rather than a stock card with a logo dropped on
 * it.
 *
 * Run: node tools/og.mjs
 * Output: web/public/og/*.png at 1200x630.
 */

import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'

const ROOT = path.resolve(new URL('..', import.meta.url).pathname)
const OUT = path.join(ROOT, 'web/public/og')
const FONTS = path.join(ROOT, 'web/public/fonts')

/** Every card the site references, keyed by the `og` prop pages pass. */
const CARDS = [
  {
    slug: 'default',
    title: 'Every step your agent took, and what it cost.',
    legend: 'Agent observability',
    note: 'docker run -p 4317:4317 ghcr.io/mcpd/mcpd',
  },
  {
    slug: 'docs',
    title: 'Documentation',
    legend: 'mcpd · docs',
    note: 'Traces, signals, SDKs, deployment and the full reference.',
  },
  {
    slug: 'quickstart',
    title: 'Five minutes to your first trace.',
    legend: 'mcpd · quickstart',
    note: 'One container, no database, no account.',
  },
  {
    slug: 'app',
    title: 'Read the run.',
    legend: 'mcpd · interface',
    note: 'Every model call and tool call, with the prompt and the cost.',
  },
  {
    slug: 'deploy',
    title: 'The same binary on a laptop and on a cluster.',
    legend: 'mcpd · deploy',
    note: 'Docker, Kubernetes, Helm.',
  },
]

/** The chart trace, generated the same way the site's instrument is. */
function tracePath(w, h) {
  const pts = []
  for (let x = 0; x <= w; x += 6) {
    const t = x / w
    let y =
      Math.sin(t * Math.PI * 2 * 3) * h * 0.1 +
      Math.sin(t * Math.PI * 2 * 7) * h * 0.05 +
      Math.sin(t * Math.PI * 2 * 13) * h * 0.025
    for (let s = 0; s < 4; s++) {
      const centre = ((s + 0.5) / 4) * w
      const width = 34
      const d = Math.abs(x - centre)
      if (d < width) {
        const fall = 1 - d / width
        y -= fall * fall * h * 0.3
      }
    }
    pts.push(`${x.toFixed(0)},${(h * 0.62 + y).toFixed(1)}`)
  }
  return `M ${pts.join(' L ')}`
}

function html(card) {
  const b64 = (f) => fs.readFileSync(path.join(FONTS, f)).toString('base64')
  return `<!doctype html><html><head><meta charset="utf-8"><style>
@font-face{font-family:'Technor';src:url(data:font/woff2;base64,${b64('technor.woff2')}) format('woff2-variations');font-weight:200 900}
@font-face{font-family:'Supreme';src:url(data:font/woff2;base64,${b64('supreme.woff2')}) format('woff2-variations');font-weight:100 800}
@font-face{font-family:'Tabular';src:url(data:font/woff2;base64,${b64('tabular.woff2')}) format('woff2-variations');font-weight:300 700}
*{margin:0;padding:0;box-sizing:border-box}
body{width:1200px;height:630px;background:#d4d5ca;color:#191a15;font-family:'Supreme',sans-serif;
 background-image:repeating-linear-gradient(0deg,rgba(25,26,21,.014) 0 1px,transparent 1px 3px);
 display:flex;flex-direction:column;overflow:hidden}
.rail{display:flex;align-items:baseline;gap:14px;padding:38px 56px 0}
.mark{font-family:'Technor';font-weight:600;font-size:34px;letter-spacing:-.03em}
.legend{font-family:'Technor';font-weight:600;font-size:13px;letter-spacing:.2em;
 text-transform:uppercase;color:#6a6c5f}
.seam{margin:26px 56px 0;border-top:1px solid rgba(25,26,21,.16);box-shadow:0 1px 0 rgba(255,255,255,.66)}
.body{flex:1;padding:44px 56px 0}
h1{font-family:'Technor';font-weight:600;font-size:${card.title.length > 44 ? 62 : 74}px;
 line-height:1.02;letter-spacing:-.025em;max-width:19ch}
.note{margin-top:26px;font-family:'Tabular';font-size:20px;color:#43453b}
.chart{height:180px;margin:0 56px 44px;background:#edeee5;
 border:1px solid rgba(25,26,21,.16);border-bottom-color:rgba(255,255,255,.66);position:relative;overflow:hidden}
.rules{position:absolute;inset:0;
 background-image:repeating-linear-gradient(to bottom,rgba(25,26,21,.09) 0 1px,transparent 1px 26px),
                  repeating-linear-gradient(to right,rgba(25,26,21,.09) 0 1px,transparent 1px 26px)}
</style></head><body>
<div class="rail"><span class="mark">mcpd</span><span class="legend">${card.legend}</span></div>
<div class="seam"></div>
<div class="body"><h1>${card.title}</h1><p class="note">${card.note}</p></div>
<div class="chart"><div class="rules"></div>
 <svg width="1088" height="180" viewBox="0 0 1088 180" style="position:absolute;inset:0">
  <path d="${tracePath(1088, 180)}" fill="none" stroke="#191a15" stroke-width="1.6" stroke-linejoin="round"/>
 </svg>
</div>
</body></html>`
}

async function loadChromium() {
  const require = createRequire(import.meta.url)
  for (const spec of ['playwright', 'playwright-core']) {
    try {
      return require(spec).chromium
    } catch { /* try the next one */ }
  }
  // Fall back to any sibling checkout that already has it, so this tool works
  // without adding a heavy dependency to the site's own package.
  for (const base of ['/home/user/prod/work-ideas', '/home/user/prod/comply.wiki']) {
    try {
      return (await import(`${base}/node_modules/playwright/index.mjs`)).chromium
    } catch { /* try the next one */ }
  }
  throw new Error('playwright is not installed; run `npm i -D playwright` in web/')
}

const chromium = await loadChromium()
fs.mkdirSync(OUT, { recursive: true })

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 })
for (const card of CARDS) {
  await page.setContent(html(card), { waitUntil: 'load' })
  await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: path.join(OUT, `${card.slug}.png`) })
  console.log(`  og/${card.slug}.png`)
}
await browser.close()
console.log(`\nwrote ${CARDS.length} share images`)
