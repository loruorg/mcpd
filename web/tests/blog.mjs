#!/usr/bin/env node
/**
 * Reads a build made with `tests/fixtures/blog/every-key.md` copied into
 * `src/content/blog/`, and checks that every front matter key the blog accepts
 * reached the page, the index, the feed and the sitemap.
 *
 *   cp tests/fixtures/blog/every-key.md src/content/blog/ && npm run build && node tests/blog.mjs
 *
 * Posts are committed by an outside writer and published without review, so
 * this is the only place a key that stopped rendering is noticed.
 */
import fs from 'node:fs'
import path from 'node:path'

const DIST = path.resolve(new URL('..', import.meta.url).pathname, 'dist')
const read = (rel) => {
  const file = path.join(DIST, rel)
  if (!fs.existsSync(file)) {
    console.error(`blog: ${rel} was not built. Copy the fixture into src/content/blog and build first.`)
    process.exit(1)
  }
  return fs.readFileSync(file, 'utf8')
}

const problems = []
const expect = (ok, what) => {
  if (!ok) problems.push(what)
}

const post = read('blog/every-key.html')
const index = read('blog.html')
const feed = read('blog/rss.xml')
const sitemap = read('sitemap-0.xml')

const h1s = post.match(/<h1[\s>][\s\S]*?<\/h1>/g) ?? []
expect(h1s.length === 1, `the post has ${h1s.length} h1 elements; it needs exactly one`)
expect(h1s[0]?.includes('Every front matter key: a test post'), 'the h1 is not the title')
expect(post.includes('<title>Every front matter key: a test post · mcpd</title>'), 'the <title> is not the title')
expect(post.includes('content="A post that sets every front matter key'), 'the description is not the meta description')
expect(post.includes('<link rel="canonical" href="https://mcpd.sh/blog/every-key">'), 'the canonical is not /blog/every-key')

expect(post.includes('<time datetime="2026-10-01">1 October 2026</time>'), 'publishDate is not shown')
expect(post.includes('<time datetime="2026-10-15">15 October 2026</time>'), 'updatedDate is not shown')
expect(/\d+ min read/.test(post), 'the reading time is not shown')
expect(post.includes('Test Author'), 'author is not shown')
expect(post.includes('testing · tracing'), 'tags are not shown')
expect(post.includes('<meta property="article:tag" content="tracing">'), 'tags are not in article:tag')
expect(post.includes('<meta property="article:published_time" content="2026-10-01">'), 'article:published_time is missing')

expect(/<img src="https:\/\/mcpd\.sh\/og\/default\.png" alt="The mcpd share card"/.test(post), 'the hero image is missing or has no alt')
expect(post.includes('<meta property="og:image" content="https://mcpd.sh/og/default.png">'), 'image is not the og:image')
expect(post.includes('<meta property="og:type" content="article">'), 'og:type is not article')
expect(post.includes('<meta name="twitter:card" content="summary_large_image">'), 'twitter:card is missing')

const ld = [...post.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => m[1])
expect(ld.some((s) => s.includes('"x-fixture":"schema-json-verbatim"')), 'schema_json is not printed verbatim')
expect(ld.filter((s) => s.includes('"BlogPosting"')).length === 1, 'schema_json did not replace the built BlogPosting')
expect(!ld.some((s) => s.includes('"FAQPage"')), 'a FAQPage was built although schema_json is set')
expect(ld.some((s) => s.includes('"BreadcrumbList"')), 'BreadcrumbList is missing')
for (const s of ld) {
  try {
    JSON.parse(s)
  } catch {
    problems.push(`a JSON-LD block does not parse: ${s.slice(0, 80)}`)
  }
}

expect(post.includes('Does the FAQ render?') && post.includes('A blank line starts the second one.'), 'the FAQ is not rendered')
expect(/<h2[^>]*>A second-level heading<\/h2>/.test(post), 'body h2 is missing')
expect(/<h3[^>]*>A third-level heading<\/h3>/.test(post), 'body h3 is missing')
expect(post.includes('<div class="table-scroll"><table'), 'the table is not in a scroll frame')
expect(post.includes('<div class="code-well"><pre class="astro-code'), 'the code block is not in a well')
expect(post.includes('<blockquote>'), 'the quotation is missing')
expect(post.includes('data-footnotes'), 'the footnotes are missing')
expect(post.includes('href="/app"'), 'the call to action is missing')

expect(!/name="robots" content="[^"]*noindex/.test(index), 'the index is noindex although a post exists')
// Real posts share the build, so the fixture may sit on a later index page,
// or past the 50 newest posts the feed carries.
const pages = fs.existsSync(path.join(DIST, 'blog/page'))
  ? fs.readdirSync(path.join(DIST, 'blog/page')).map((f) => read(`blog/page/${f}`))
  : []
expect([index, ...pages].some((p) => p.includes('href="/blog/every-key"')), 'no index page lists the post')
expect(
  feed.includes('<link>https://mcpd.sh/blog/every-key</link>') || (feed.match(/<item>/g) ?? []).length === 50,
  'the feed does not list the post',
)
expect(sitemap.includes('<loc>https://mcpd.sh/blog/every-key</loc>'), 'the sitemap does not list the post')
expect(sitemap.includes('<loc>https://mcpd.sh/blog</loc>'), 'the sitemap does not list the index')

if (problems.length) {
  for (const p of problems) console.error(`blog: ${p}`)
  process.exit(1)
}
console.log('blog: every front matter key renders')
