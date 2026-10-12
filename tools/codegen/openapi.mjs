import { parseType, pascal, allObjects } from './lib.mjs'

const PRIM = {
  string: { type: 'string' }, bool: { type: 'boolean' },
  f64: { type: 'number', format: 'double' },
  u64: { type: 'integer', format: 'int64', minimum: 0 },
  u32: { type: 'integer', format: 'int32', minimum: 0 },
  i64: { type: 'integer', format: 'int64' },
  json: {}, timestamp: { type: 'string', format: 'date-time' },
}

function schemaFor(t) {
  const { base, array } = parseType(t)
  let s
  if (PRIM[base]) {
    s = { ...PRIM[base] }
  } else if (base.startsWith('otlp.')) {
    // An OTLP message is defined by opentelemetry-proto, not by the spec, and
    // a `$ref` to a schema this document does not carry is a broken document:
    // a client generator stops on it. Name the message and leave it open.
    const message = base.slice('otlp.'.length)
    s = {
      type: 'object',
      description: `The OpenTelemetry ${message} message, as defined by opentelemetry-proto rather than by this document.`,
      'x-otlp-message': message,
    }
  } else {
    s = { $ref: `#/components/schemas/${pascal(base)}` }
  }
  if (array) s = { type: 'array', items: s }
  return s
}

export function emitOpenApi(spec) {
  const schemas = {}
  for (const [name, e] of Object.entries(spec.enums)) {
    schemas[pascal(name)] = {
      type: 'string', description: e.doc,
      enum: e.values.map(v => v.name),
      'x-enumDescriptions': Object.fromEntries(e.values.map(v => [v.name, v.doc])),
    }
  }
  for (const { name, def } of allObjects(spec)) {
    const props = {}, required = []
    for (const f of def.fields) {
      const p = parseType(f.type)
      props[f.name] = { ...schemaFor(f.type), description: f.doc }
      if (f.default !== undefined) props[f.name].default = f.default
      if (!p.optional) required.push(f.name)
    }
    schemas[pascal(name)] = {
      type: 'object', description: def.doc, properties: props,
      ...(required.length ? { required } : {}),
      ...(def.table ? { 'x-table': def.table } : {}),
    }
  }

  const paths = {}
  for (const ep of spec.endpoints) {
    const p = ep.path.replace(/\{(\w+)\}/g, '{$1}')
    paths[p] ??= {}
    const params = []
    for (const q of ep.params ?? []) {
      params.push({ name: q.name, in: 'path', required: true, description: q.doc, schema: schemaFor(q.type) })
    }
    for (const q of ep.query ?? []) {
      const t = parseType(q.type)
      params.push({
        name: q.name, in: 'query', required: !t.optional, description: q.doc,
        // OpenAPI's default for an array is a repeated key, `tags=a&tags=b`.
        // Most endpoints read the query as a map and keep only the last one,
        // and every endpoint splits on commas, so a generated client has to
        // send `tags=a,b`.
        ...(t.array ? { style: 'form', explode: false } : {}),
        schema: { ...schemaFor(q.type), ...(q.default !== undefined ? { default: q.default } : {}) },
      })
    }
    paths[p][ep.method.toLowerCase()] = {
      operationId: ep.id, tags: [ep.tag], summary: ep.summary, description: ep.doc,
      ...(params.length ? { parameters: params } : {}),
      ...(ep.request ? {
        requestBody: {
          required: true,
          content: { 'application/json': { schema: schemaFor(ep.request.type) } },
        },
      } : {}),
      responses: {
        '200': {
          description: 'Success',
          content: { 'application/json': { schema: schemaFor(ep.response.type) } },
        },
        '400': { description: 'The request was malformed. The body names the field and the problem.' },
        '401': { description: 'No credential, or one this endpoint does not accept.' },
        '429': { description: 'Rate limited. `Retry-After` says how long to wait.' },
        // Declared per endpoint. A declared status overrides the shared text
        // above, so an endpoint can say what its own 400 means.
        ...Object.fromEntries(Object.entries(ep.responses ?? {}).map(([code, r]) => [code, {
          description: r.doc,
          ...(r.type ? { content: { 'application/json': { schema: schemaFor(r.type) } } } : {}),
        }])),
      },
      security: ep.auth === 'none' ? [] : [{ bearerAuth: [] }],
    }
  }

  return JSON.stringify({
    openapi: '3.1.0',
    info: {
      title: 'mcpd',
      version: spec.specVersion,
      description: spec.doc,
      license: { name: 'Apache-2.0', identifier: 'Apache-2.0' },
    },
    servers: [{ url: 'http://localhost:4317', description: 'A local mcpd' }],
    components: {
      schemas,
      securitySchemes: {
        bearerAuth: {
          type: 'http', scheme: 'bearer',
          description: 'An mcpd API key. Each operation names the scope it needs: ingest, read, write, admin, execute or worker.',
        },
      },
    },
    paths,
  }, null, 2) + '\n'
}
