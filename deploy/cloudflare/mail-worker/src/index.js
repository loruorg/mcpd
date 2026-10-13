// POST /send with `Authorization: Bearer <MAIL_TOKEN>` and a JSON body
// { "to": "...", "subject": "...", "text": "...", "html": "..." }.
// The sender is fixed by MAIL_FROM, so a leaked token cannot send as anyone
// else.

const MAX_BODY = 64 * 1024

function equal(a, b) {
  const x = new TextEncoder().encode(a)
  const y = new TextEncoder().encode(b)
  if (x.length !== y.length) return false
  let d = 0
  for (let i = 0; i < x.length; i++) d |= x[i] ^ y[i]
  return d === 0
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url)
    if (url.pathname !== '/send') return new Response('not found', { status: 404 })
    if (request.method !== 'POST') return new Response('method not allowed', { status: 405 })
    const auth = request.headers.get('authorization') ?? ''
    if (!env.MAIL_TOKEN || !equal(auth, `Bearer ${env.MAIL_TOKEN}`)) {
      return new Response('unauthorized', { status: 401 })
    }
    const raw = await request.text()
    if (raw.length > MAX_BODY) return new Response('too large', { status: 413 })
    let mail
    try {
      mail = JSON.parse(raw)
    } catch {
      return new Response('body is not JSON', { status: 400 })
    }
    const { to, subject, text, html } = mail ?? {}
    if (typeof to !== 'string' || !to.includes('@') || typeof subject !== 'string' || typeof text !== 'string') {
      return new Response('to, subject and text are required', { status: 400 })
    }
    try {
      await env.EMAIL.send({ from: env.MAIL_FROM, to, subject, text, ...(typeof html === 'string' ? { html } : {}) })
    } catch (e) {
      return Response.json({ error: 'send_failed', message: String(e?.message ?? e) }, { status: 502 })
    }
    return Response.json({ sent: true })
  },
}
