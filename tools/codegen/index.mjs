#!/usr/bin/env node
// The one generator. Reads spec/mcpd.spec.json, writes every derived artifact.
//
//   node tools/codegen            regenerate everything
//   node tools/codegen --check    exit 1 if anything would change (CI drift gate)
import { loadSpec, write, ROOT } from './lib.mjs'
import { emitRust } from './rust.mjs'
import { emitTypeScript } from './typescript.mjs'
import { emitPython } from './python.mjs'
import { emitGo } from './go.mjs'
import { emitKotlin } from './kotlin.mjs'
import { emitElixir } from './elixir.mjs'
import { emitOpenApi } from './openapi.mjs'
import { emitDataModelDoc, emitApiDocs, emitDocsIndex } from './docs.mjs'
import fs from 'node:fs'
import path from 'node:path'

const CHECK = process.argv.includes('--check')
const spec = loadSpec()

const outputs = [
  ['crates/mcpd-core/src/generated.rs', emitRust(spec)],
  ['sdks/typescript/src/generated.ts', emitTypeScript(spec)],
  // The operator interface is TypeScript too, and must never drift from the
  // SDK's view of the model. Same emitter, second destination.
  ['web/src/generated/types.ts', emitTypeScript(spec, { wire: true })],
  ['sdks/python/mcpd/_generated.py', emitPython(spec)],
  ['sdks/go/generated.go', emitGo(spec)],
  ['sdks/kotlin/src/main/kotlin/dev/mcpd/Generated.kt', emitKotlin(spec)],
  ['sdks/elixir/lib/mcpd/generated.ex', emitElixir(spec)],
  ['spec/openapi.json', emitOpenApi(spec)],
  ['web/src/content/reference/data-model.md', emitDataModelDoc(spec)],
  ['web/src/generated/spec-index.json', emitDocsIndex(spec)],
]
for (const f of emitApiDocs(spec)) {
  outputs.push([`web/src/content/reference/api-${f.slug}.md`, f.content])
}

let changed = []
for (const [rel, content] of outputs) {
  if (CHECK) {
    const p = path.join(ROOT, rel)
    const prev = fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null
    if (prev !== content) changed.push(rel)
  } else {
    const r = write(rel, content)
    if (r.changed) changed.push(rel)
  }
}

if (CHECK) {
  if (changed.length) {
    console.error('Generated files are out of date with spec/mcpd.spec.json:\n')
    for (const c of changed) console.error('  ' + c)
    console.error('\nRun `make gen` and commit the result.')
    process.exit(1)
  }
  console.log(`codegen: ${outputs.length} artifacts up to date with spec ${spec.specVersion}`)
} else {
  console.log(`codegen: wrote ${outputs.length} artifacts from spec ${spec.specVersion}` +
    (changed.length ? ` (${changed.length} changed)` : ' (no changes)'))
  for (const c of changed) console.log('  ~ ' + c)
}
