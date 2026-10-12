import { parseType, pascal, allObjects, BANNER_LINES } from './lib.mjs'

const frontmatter = (title, description, order) =>
  `---\n` +
  `title: ${JSON.stringify(title)}\n` +
  `description: ${JSON.stringify(description)}\n` +
  `order: ${order}\n` +
  `generated: true\n` +
  `---\n\n` +
  `<!-- ${BANNER_LINES.join(' ')} -->\n\n`

function typeLink(t) {
  const { base, array, optional } = parseType(t)
  const prim = ['string','bool','f64','u64','u32','i64','json','timestamp']
  const label = prim.includes(base) ? `\`${base}\`` : `[\`${pascal(base)}\`](#${base.toLowerCase()})`
  return `${label}${array ? '[]' : ''}${optional ? '' : ' <span class="req">required</span>'}`
}

/** The data-model reference: every enum, type and entity, with its prose. */
export function emitDataModelDoc(spec) {
  const o = [frontmatter(
    'Data model',
    'Every entity, field and enumerated value mcpd stores, generated directly from the specification, so it can never drift from the running server.',
    1,
  )]
  o.push('mcpd stores five signals: spans, logs, metrics, scores and feedback. Every one of them is correlated by trace and span id. This page is generated from `spec/mcpd.spec.json`, the same file the server and all six SDKs are built from. If it says a field exists, it exists.\n')

  o.push('## Enumerations\n')
  for (const [name, e] of Object.entries(spec.enums)) {
    o.push(`### ${pascal(name)}\n`)
    o.push(`${e.doc}\n`)
    o.push('| Value | Meaning |')
    o.push('| --- | --- |')
    for (const v of e.values) o.push(`| \`${v.name}\` | ${v.doc} |`)
    o.push('')
  }

  o.push('## Entities\n')
  for (const { name, def, kind } of allObjects(spec)) {
    o.push(`### ${pascal(name)}\n`)
    o.push(`${def.doc}\n`)
    if (def.table) o.push(`Stored in the \`${def.table}\` table.\n`)
    o.push('| Field | Type | Description |')
    o.push('| --- | --- | --- |')
    for (const f of def.fields) o.push(`| \`${f.name}\` | ${typeLink(f.type)} | ${f.doc} |`)
    o.push('')
    if (def.primaryKey) o.push(`**Primary key:** ${def.primaryKey.map(k => `\`${k}\``).join(', ')}\n`)
    if (def.indexes?.length) {
      o.push('**Indexes**\n')
      for (const ix of def.indexes) o.push(`- \`${ix.on.join(', ')}\`: ${ix.doc}`)
      o.push('')
    }
  }
  return o.join('\n')
}

/** One reference page per API tag. */
export function emitApiDocs(spec) {
  const byTag = {}
  for (const ep of spec.endpoints) (byTag[ep.tag] ??= []).push(ep)

  const files = []
  let order = 2
  for (const [tag, eps] of Object.entries(byTag)) {
    const o = [frontmatter(
      tag,
      `The ${tag.toLowerCase()} endpoints of the mcpd HTTP API.`,
      order++,
    )]
    for (const ep of eps) {
      o.push(`## ${ep.summary}\n`)
      o.push(`<div class="endpoint"><span class="method method--${ep.method.toLowerCase()}">${ep.method}</span><code>${ep.path}</code></div>\n`)
      o.push(`${ep.doc}\n`)
      if (ep.params?.length) {
        o.push('**Path parameters**\n')
        o.push('| Name | Type | Description |', '| --- | --- | --- |')
        for (const p of ep.params) o.push(`| \`${p.name}\` | \`${parseType(p.type).base}\` | ${p.doc} |`)
        o.push('')
      }
      if (ep.query?.length) {
        o.push('**Query parameters**\n')
        o.push('| Name | Type | Default | Description |', '| --- | --- | --- | --- |')
        for (const q of ep.query) {
          const t = parseType(q.type)
          o.push(`| \`${q.name}\` | \`${t.base}${t.array ? '[]' : ''}\` | ${q.default !== undefined ? `\`${q.default}\`` : t.optional ? '·' : '**required**'} | ${q.doc} |`)
        }
        o.push('')
      }
      if (ep.request) o.push(`**Request body:** \`${pascal(parseType(ep.request.type).base)}\`\n`)
      o.push(`**Response:** \`${pascal(parseType(ep.response.type).base)}\`\n`)
      // The statuses this endpoint answers beyond 200 and the three every
      // endpoint shares. A 409 that carries a typed body names the type, so a
      // worker reading this knows what to parse before it knows what to do.
      const extra = Object.entries(ep.responses ?? {}).sort(([a], [b]) => a.localeCompare(b))
      if (extra.length) {
        o.push('**Other responses**\n')
        o.push('| Status | Body | When |', '| --- | --- | --- |')
        for (const [code, r] of extra) {
          o.push(`| \`${code}\` | ${r.type ? `\`${pascal(parseType(r.type).base)}\`` : '·'} | ${r.doc} |`)
        }
        o.push('')
      }
      const authText =
        ep.auth === 'none'
          ? 'none; this endpoint is public.'
          : ep.auth === 'session'
            ? 'an account session token from `POST /v1/api/account/login`. An API key is not accepted.'
            : ep.auth === 'scim'
              ? 'the organisation\'s SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.'
              : `an API key with \`${ep.auth}\` scope.`
      o.push(`**Authentication:** ${authText}\n`)
      o.push('---\n')
    }
    files.push({ slug: tag.toLowerCase().replace(/\s+/g, '-'), content: o.join('\n') })
  }
  return files
}

/** A compact JSON index the docs site uses to render nav and search. */
export function emitDocsIndex(spec) {
  return JSON.stringify({
    specVersion: spec.specVersion,
    apiVersion: spec.apiVersion,
    counts: {
      enums: Object.keys(spec.enums).length,
      types: Object.keys(spec.types).length,
      entities: Object.keys(spec.entities).length,
      endpoints: spec.endpoints.length,
      spanKinds: spec.enums.SpanKind.values.length,
    },
    spanKinds: spec.enums.SpanKind.values.map(v => ({ name: v.name, doc: v.doc })),
    endpoints: spec.endpoints.map(e => ({ id: e.id, tag: e.tag, method: e.method, path: e.path, summary: e.summary })),
  }, null, 2) + '\n'
}
