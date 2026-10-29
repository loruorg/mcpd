import fs from 'node:fs'
import { defineCollection } from 'astro:content'
import { z } from 'astro/zod'
import { glob, type Loader } from 'astro/loaders'

/**
 * Generated reference pages. Everything in this collection is produced by
 * `tools/codegen` from `spec/mcpd.spec.json` and must never be edited by hand;
 * `make check` fails when one of these files differs from what the spec
 * produces.
 */
const reference = defineCollection({
  loader: glob({ pattern: '**/*.md', base: './src/content/reference' }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    order: z.number().default(99),
    generated: z.boolean().default(false),
  }),
})

/* A writer that leaves a key empty writes `key:` or `key: ""`. Both mean
   "not set", and refusing them would fail the deploy that publishes the post. */
const blank = (v: unknown) => (v === null || v === '' ? undefined : v)
const optional = <T extends z.ZodType>(schema: T) => z.preprocess(blank, schema.optional())

/**
 * Blog posts: one Markdown file per post, `src/content/blog/<slug>.md`,
 * published at `/blog/<slug>`.
 *
 * Posts are committed to `main` by an outside writer and go live with the
 * deploy that the push starts, with nobody reading them first. So the schema
 * names the keys the pages read and nothing more: a key it does not name is
 * dropped, not refused, and a post never fails the build for carrying one.
 */
const posts = glob({
  pattern: '*.md',
  base: './src/content/blog',
  // The file name is the slug. The default reads a `slug` key from the front
  // matter first, so a post could publish under a URL its file does not name.
  generateId: ({ entry }) => entry.replace(/\.md$/, ''),
})

/* The glob loader returns early when the folder holds no file, and leaves the
   previous build's entries in its cache. Deleting the last post would then
   keep it published on any machine that built with it before. */
const blogLoader: Loader = {
  ...posts,
  async load(context) {
    await posts.load(context)
    const dir = new URL('./src/content/blog/', context.config.root)
    if (!fs.readdirSync(dir).some((f) => f.endsWith('.md'))) context.store.clear()
  },
}

const blog = defineCollection({
  loader: blogLoader,
  schema: z.object({
    title: z.string(),
    description: z.string(),
    // YAML reads an unquoted 2026-10-30 as a date and a quoted one as a string.
    publishDate: z.coerce.date(),
    updatedDate: optional(z.coerce.date()),
    tags: z.preprocess((v) => blank(v) ?? [], z.array(z.string())),
    faq: optional(z.array(z.object({ question: z.string(), answer: z.string() }))),
    author: optional(z.string()),
    image: optional(z.string()),
    image_alt: optional(z.string()),
    // JSON-LD, already escaped for a <script> element. Printed as it is.
    schema_json: optional(z.string()),
  }),
})

export const collections = { reference, blog }
