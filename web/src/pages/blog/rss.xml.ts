/**
 * The blog's RSS 2.0 feed at `/blog/rss.xml`, newest 50 posts. With no posts it
 * is a valid, empty channel.
 */
import type { APIRoute } from 'astro'
import { SITE } from '@/lib/site'
import { BLOG, absolute, allPosts, postPath } from '@/lib/blog'

const xml = (s: string) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

export const GET: APIRoute = async () => {
  const posts = (await allPosts()).slice(0, 50)
  const items = posts.map((p) => {
    const url = absolute(postPath(p.id))
    return [
      '<item>',
      `<title>${xml(p.data.title)}</title>`,
      `<link>${url}</link>`,
      `<guid isPermaLink="true">${url}</guid>`,
      `<pubDate>${p.data.publishDate.toUTCString()}</pubDate>`,
      `<description>${xml(p.data.description)}</description>`,
      ...p.data.tags.map((t) => `<category>${xml(t)}</category>`),
      '</item>',
    ].join('')
  })
  const changed = posts.map((p) => (p.data.updatedDate ?? p.data.publishDate).getTime())
  const newest = changed.length ? new Date(Math.max(...changed)) : undefined
  const body = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel>',
    `<title>${xml(`${SITE.name} ${BLOG.title.toLowerCase()}`)}</title>`,
    `<link>${absolute(BLOG.path)}</link>`,
    `<description>${xml(BLOG.description)}</description>`,
    '<language>en</language>',
    `<atom:link href="${absolute(BLOG.feed)}" rel="self" type="application/rss+xml"/>`,
    ...(newest ? [`<lastBuildDate>${newest.toUTCString()}</lastBuildDate>`] : []),
    ...items,
    '</channel></rss>',
  ].join('\n')
  return new Response(body, { headers: { 'Content-Type': 'application/rss+xml; charset=utf-8' } })
}
