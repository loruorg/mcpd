/**
 * The blog's routes, ordering and dates. Read by the index, the post page and
 * the feed, so the three cannot disagree about which posts exist or where they
 * live.
 */
import { getCollection, type CollectionEntry } from 'astro:content'
import { SITE } from '@/lib/site'

export type Post = CollectionEntry<'blog'>

export const BLOG = {
  path: '/blog',
  feed: '/blog/rss.xml',
  title: 'Blog',
  description: 'Articles on tracing, testing and running AI agents with mcpd.',
} as const

/** Posts on one index page. Past this the index paginates. */
export const PAGE_SIZE = 20

/** Every post, newest first. Posts from the same day fall back to their slug, so a rebuild never reorders them. */
export async function allPosts(): Promise<Post[]> {
  const posts = await getCollection('blog')
  return posts.sort(
    (a, b) => b.data.publishDate.getTime() - a.data.publishDate.getTime() || a.id.localeCompare(b.id),
  )
}

export const postPath = (slug: string) => `${BLOG.path}/${slug}`

/** Page 1 is the index itself; the rest sit under `/blog/page/`, where no post slug can take them. */
export const indexPath = (page: number) => (page <= 1 ? BLOG.path : `${BLOG.path}/page/${page}`)

export const absolute = (pathOrUrl: string) => new URL(pathOrUrl, SITE.url).href

/* Dates are calendar days with no time zone. YAML and `new Date('2026-10-30')`
   both give UTC midnight, so they are read back in UTC, or a reader west of
   Greenwich would see the day before. */
export const isoDay = (d: Date) => d.toISOString().slice(0, 10)
export const longDay = (d: Date) =>
  d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' })

/** Minutes to read a Markdown body at 230 words a minute, never less than one. */
export function readingMinutes(markdown: string): number {
  const text = markdown
    .replace(/<[^>]+>/g, ' ')
    .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[#>*_`|~-]+/g, ' ')
  const words = text.split(/\s+/).filter(Boolean).length
  return Math.max(1, Math.round(words / 230))
}

/** Up to `n` other posts, most shared tags first, then newest. */
export function relatedPosts(post: Post, posts: Post[], n = 3): Post[] {
  // Tags compare as keys, so the fold is locale-invariant.
  const mine = new Set(post.data.tags.map((t) => t.toLowerCase()))
  const shared = (p: Post) => p.data.tags.filter((t) => mine.has(t.toLowerCase())).length
  return posts
    .filter((p) => p.id !== post.id)
    .map((p, i) => ({ p, i, s: shared(p) }))
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .slice(0, n)
    .map(({ p }) => p)
}

/**
 * JSON for a `<script type="application/ld+json">` element. Titles and answers
 * come from a post, and a `</script>` inside one would end the element early.
 */
export const jsonLd = (value: unknown) => JSON.stringify(value).replace(/</g, '\\u003c')
