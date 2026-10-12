import { defineCollection } from 'astro:content'
import { z } from 'astro/zod'
import { glob } from 'astro/loaders'

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

export const collections = { reference }
