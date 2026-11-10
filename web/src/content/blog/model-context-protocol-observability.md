---
description: "MCP tool failures pass uptime checks because isError results return HTTP 200. Learn model context protocol observability: one trace joins server and agent."
faq:
  - answer: "Point an OpenTelemetry exporter at a collector or at mcpd with OTEL_EXPORTER_OTLP_ENDPOINT, record each tool call as a span using gen_ai.* attributes, carry traceparent across the client-server boundary, and mark isError results as errored spans. Then read each run's prompt, arguments and result."
    question: How do I monitor an MCP server in production?
  - answer: Observability with mcpd adds no failure point to the MCP path, because mcpd is a process, not a proxy, and nothing travels through it to reach a model or an MCP server. If mcpd is slow or down, your tool calls are unaffected and your agent keeps working.
    question: Does observability add a failure point to the MCP path?
  - answer: "The difference is direction: mcpd mcp is for an agent reading from mcpd, while instrumenting MCP calls sends your spans into mcpd. mcpd mcp exposes find_runs, get_run and run_stats over stdio so an agent can read traces. The integrating docs compare the two routes."
    question: What is the difference between mcpd mcp and instrumenting MCP calls?
  - answer: Logs from a stdio MCP server go to standard error, where the server may write UTF-8 log strings and the client may capture, forward or ignore them. Standard output is reserved for valid MCP messages, so never print logs to standard output.
    question: Where do MCP server logs go over stdio?
  - answer: You can use your existing OpenTelemetry or OpenInference instrumentation with mcpd, because mcpd reads both the gen_ai.* attributes and the OpenInference llm.* attributes. To use it, point your current exporter at mcpd and leave your instrumentation code exactly as it is.
    question: Can I use existing OpenTelemetry or OpenInference instrumentation?
image: /blog/model-context-protocol-observability/featured-e4501582382a.webp
image_alt: "Illustration for Model context protocol observability: Trace MCP calls"
publishDate: "2026-11-10"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/model-context-protocol-observability#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\",\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]}],\"datePublished\":\"2026-11-10\",\"description\":\"MCP tool failures pass uptime checks because isError results return HTTP 200. Learn model context protocol observability: one trace joins server and agent.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Point an OpenTelemetry exporter at a collector or at mcpd with OTEL_EXPORTER_OTLP_ENDPOINT, record each tool call as a span using gen_ai.* attributes, carry traceparent across the client-server boundary, and mark isError results as errored spans. Then read each run's prompt, arguments and result.\"},\"name\":\"How do I monitor an MCP server in production?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Observability with mcpd adds no failure point to the MCP path, because mcpd is a process, not a proxy, and nothing travels through it to reach a model or an MCP server. If mcpd is slow or down, your tool calls are unaffected and your agent keeps working.\"},\"name\":\"Does observability add a failure point to the MCP path?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The difference is direction: mcpd mcp is for an agent reading from mcpd, while instrumenting MCP calls sends your spans into mcpd. mcpd mcp exposes find_runs, get_run and run_stats over stdio so an agent can read traces. The integrating docs compare the two routes.\"},\"name\":\"What is the difference between mcpd mcp and instrumenting MCP calls?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Logs from a stdio MCP server go to standard error, where the server may write UTF-8 log strings and the client may capture, forward or ignore them. Standard output is reserved for valid MCP messages, so never print logs to standard output.\"},\"name\":\"Where do MCP server logs go over stdio?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can use your existing OpenTelemetry or OpenInference instrumentation with mcpd, because mcpd reads both the gen_ai.* attributes and the OpenInference llm.* attributes. To use it, point your current exporter at mcpd and leave your instrumentation code exactly as it is.\"},\"name\":\"Can I use existing OpenTelemetry or OpenInference instrumentation?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q28865\",\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\",\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q1164443\",\"@type\":\"Thing\",\"description\":\"JSON-based wire protocol for remote procedure calls\",\"name\":\"JSON-RPC\",\"sameAs\":[\"https://en.wikipedia.org/wiki/JSON-RPC\",\"https://www.wikidata.org/wiki/Q1164443\",\"https://www.google.com/search?kgmid=/m/06wb0l\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\",\"@type\":\"Thing\",\"description\":\"application-layer protocol for transferring web resources\",\"name\":\"HTTP\",\"sameAs\":[\"https://en.wikipedia.org/wiki/HTTP\",\"https://www.wikidata.org/wiki/Q8777\",\"https://www.google.com/search?kgmid=/m/03hgt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\",\"@type\":\"Thing\",\"description\":\"programming language, superset of JavaScript that compiles to JavaScript\",\"name\":\"TypeScript\",\"sameAs\":[\"https://en.wikipedia.org/wiki/TypeScript\",\"https://www.wikidata.org/wiki/Q978185\",\"https://www.google.com/search?kgmid=/m/0n50hxv\"]}],\"name\":\"Model context protocol observability: Trace MCP calls\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/WtnGEZjjdZrt1Pk6fWFRbvZ1J-PPDHWg.webp\"},\"url\":\"https://mcpd.sh/blog/model-context-protocol-observability\"},{\"@id\":\"https://mcpd.sh/blog/model-context-protocol-observability#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\"},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"MCP Server Observability: Logging and Tracing Tool Calls\",\"url\":\"https://inference.net/content/mcp-server-observability/\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Tools - Model Context Protocol specification\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/server/tools\"},{\"@type\":\"CreativeWork\",\"name\":\"Transports - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/basic/transports\"},{\"@type\":\"CreativeWork\",\"name\":\"Logging - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/server/utilities/logging\"},{\"@type\":\"CreativeWork\",\"name\":\"What is the Model Context Protocol (MCP)?\",\"url\":\"https://modelcontextprotocol.io/docs/2026-07-28/getting-started/intro\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context | W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"Transports - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-03-26/basic/transports\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"MCP Inspector\",\"url\":\"https://modelcontextprotocol.io/docs/tools/inspector\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"}],\"dateModified\":\"2026-11-10\",\"datePublished\":\"2026-11-10\",\"description\":\"MCP tool failures pass uptime checks because isError results return HTTP 200. Learn model context protocol observability: one trace joins server and agent.\",\"headline\":\"Model context protocol observability: Trace MCP calls\",\"image\":\"https://api.birchseek.com/m/WtnGEZjjdZrt1Pk6fWFRbvZ1J-PPDHWg.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/model-context-protocol-observability#webpage\"},\"keywords\":[\"model context protocol observability\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/model-context-protocol-observability#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q28865\"},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q1164443\"},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\"},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2562},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - mcp
  - observability
  - opentelemetry
  - tracing
  - ai agents
  - tool calls
title: "Model context protocol observability: Trace MCP calls"
---

**Model context protocol observability means joining the server's view of a tool call with the host and agent's view in one trace. Record each tool call as a span with `gen_ai.*` attributes, carry W3C trace context across the client-server boundary, and treat `isError: true` results as errors. To start, set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, or build mcpd from source.**

Your MCP-connected agent worked locally, and in production it returns a wrong answer while the server dashboard stays green: requests return 200, uptime holds, latency looks normal. Model context protocol observability gives you the run behind that answer, with the model's side of the call joined to the server's.

You will be able to find the one run behind a bad tool call, read the arguments the model generated and the result it got back, and set checks for tool-call loops, latency and cost.

> **Key Takeaways**
> - A failed tool call can arrive as a successful JSON-RPC result with `isError: true`, so [status codes and uptime checks](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) miss it.
> - A server log shows that `tools/call` arrived, not [why the model chose](https://inference.net/content/mcp-server-observability/) the tool.
> - [Redaction has to run before storage](https://mcpd.sh/docs/privacy), because a secret deleted afterwards was still written.
> - mcpd [prices each step once](https://mcpd.sh/docs/cost) at ingest, and an unknown model has no cost, not a cost of zero.

## What is model context protocol observability?

Model context protocol observability is the practice of recording both halves of an MCP tool call, joined by one trace id. One half is the server's view of the request. The other is the host and agent's view: the model prompt, the chosen tool, its arguments, the result and the next step.

![Illustration for What is model context protocol observability?](/blog/model-context-protocol-observability/image-1-2a9c37825500.webp)

[MCP describes itself](https://modelcontextprotocol.io/docs/2026-07-28/getting-started/intro) as "an open-source standard for connecting AI applications to external systems." Every tool call under it has two vantage points, and each one sees only part of the story.

### What the server sees

The server sees a `tools/call` request, a tool name, the arguments that arrived and the result it returned. [The MCP transports specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) defines two transports, stdio and Streamable HTTP, and a server using HTTP may assign a session through an `Mcp-Session-Id` header.

Over stdio, the transports page says the server "MAY write UTF-8 strings to its standard error (stderr) for logging purposes," which leaves stdout for protocol messages.

[The spec's logging utility](https://modelcontextprotocol.io/specification/2025-06-18/server/utilities/logging) adds a second channel. It gives servers "a standardized way" to send structured log messages to clients as `notifications/message`, with severity levels taken from RFC 5424.

### What the host and agent see

The host and agent see everything that led to the call: the prompt, the model's decision to call the tool, the arguments it generated, the result it received and what it did next. A server log can't supply any of that.

As [Inference's write-up on MCP server observability](https://inference.net/content/mcp-server-observability/) asks, "Why did the agent pick that tool? What arguments did the model actually generate?" The same page says of the server: "by design it never will" know.

### Joining the halves

One trace id joins the two halves. The host starts a run, and each model call and tool call becomes a span (a step) in that trace. The id travels with the request, so the server's own span and log lines land in the same trace.

Reading the run, you go from the prompt to the tool choice, then to the server's work, then to the model's next step. The [mcpd concepts guide](https://mcpd.sh/docs/concepts) defines run, trace, span and step.

A dashboard of counters reports that a tool was slow or that calls rose. In model context protocol observability, the joined trace shows why the model called the tool, with which arguments, and what it did with the answer.

## Why do MCP tool failures pass an uptime check?

An MCP tool failure passes an uptime check because the server reports it inside a successful JSON-RPC response, so the HTTP status stays 200 and the probe sees a healthy server.

[The tools section of the MCP specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) defines two error channels. Protocol errors, such as an unknown tool, use the standard JSON-RPC `error` object. Tool execution errors are "Reported in tool results with isError: true".

The spec gives this example of the second kind:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Failed to fetch weather data: API rate limit exceeded"
      }
    ],
    "isError": true
  }
}
```

A probe, a load balancer and an HTTP error-rate graph all count this response as a success. The model reads the text and may retry, apologise or invent an answer, so the agent answers wrongly while every dashboard stays green.

Read the result where you record the span. When `isError` is true, set the span status to error and add an `error.type` attribute:

```typescript
const result = await client.callTool({ name, arguments: args });
if (result.isError) {
  span.setStatus({ code: SpanStatusCode.ERROR });
  span.setAttribute("error.type", "tool_error");
}
```

Once the span carries the error, a deterministic check can read it.

For model context protocol observability, the same run also shows the arguments the model sent and what it did next.

## How do you trace MCP calls for model context protocol observability?

You trace an MCP tool call with OpenTelemetry by wrapping the call in a span that carries the GenAI attributes, then sending W3C trace context to the server so its span joins the same trace.

1. **Name the client span and set its attributes.** Put `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens` on the model-call span that chose the tool, because the tool span has no tokens of its own.

2. **Inject the context into the request.** A `traceparent` value has four dash-separated fields, as in the [W3C Trace Context](https://www.w3.org/TR/trace-context/) example `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`. Stdio has no headers, so write it into the request's `params._meta`, where the server reads it in the next step.

3. **Extract it on the server.** Read `params._meta`, rebuild the context and start the server span as its child. Do not reuse `Mcp-Session-Id` as a trace id: [the MCP transport spec](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) defines that header for the Streamable HTTP session, so it groups requests and cannot link spans.

4. **Map `isError` to an error status.** Under [the MCP tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools), a failed tool still returns a normal JSON-RPC result, so the span stays green unless you set the status yourself.

```python
from opentelemetry import trace, propagate
from opentelemetry.trace import SpanKind, Status, StatusCode

tracer = trace.get_tracer("agent")

def call_tool(send_request, name, arguments):
    with tracer.start_as_current_span(
        f"execute_tool {name}", kind=SpanKind.INTERNAL
    ) as span:
        span.set_attribute("gen_ai.operation.name", "execute_tool")
        span.set_attribute("gen_ai.tool.name", name)
        meta = {}
        propagate.inject(meta)  # writes traceparent
        result = send_request("tools/call", {
            "name": name, "arguments": arguments, "_meta": meta,
        })
        if result.get("isError"):
            span.set_status(Status(StatusCode.ERROR, "tool returned isError"))
        return result
```

5. **Send the spans and check the trace.** Set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` in both processes, as described in the [mcpd OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry). mcpd reads the `gen_ai.*` names and the OpenInference names, so instrumentation that marks tool spans with `openinference.span.kind` of `TOOL` works unchanged.

 A working setup shows one trace with the model span, the `execute_tool` span and the server span nested under it. An errored tool shows as a red span.

## How do you start model context protocol observability in five minutes?

You start model context protocol observability in five minutes by pointing an OpenTelemetry exporter at the hosted mcpd, or by building mcpd from source and running its demo. Both routes need no change to your MCP client or server code if the agent already emits OpenTelemetry.

**Hosted route**

1. Sign up at [mcpd.sh/app](https://mcpd.sh/app) and create a key from your project. The [quickstart](https://mcpd.sh/docs/quickstart) shows the keyed project setup.
2. Set two environment variables in the shell that launches your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

3. Run the agent and trigger one tool call. Open the interface and look for a new run containing the tool-call span. If the run is missing, check that the protocol is `http/json` and that the project key is set as the quickstart describes.

**Self-hosted route**

1. Install Rust and Node first. The interface is compiled into the binary, so `make build` fails without both.
2. Build and start the demo:

```shell
git clone https://github.com/loruorg/mcpd
cd mcpd
make build
./target/release/mcpd demo
```

3. Open `http://localhost:4317`.

The demo gives you a first trace to read before you wire in your own agent. Open the run and read its steps in the interface: the prompt, the tool arguments and the tool result, none of which a server log holds.

To carry the setup further, [start free on the hosted version](https://mcpd.sh/app) and send your first MCP run.

## What should you capture from a bad MCP run?

A bad MCP run leaves three pieces of evidence in its payload, and you need all three stored together, with sensitive values removed before anything is written.

| Evidence | What it shows |
|---|---|
| The arguments the model generated, the JSON it produced for the `tools/call` request | A malformed date or an invented ID, which the server log can't explain |
| The full tool result, including the `isError` flag | A failure with `isError: true` inside a successful response, visible only if you stored the body |
| The model's next step: a retry, a switch of tools or an answer that ignored the failure | Whether the error caused the wrong answer |

[mcpd's home page](https://mcpd.sh) describes the interface this way: "Open any step to read the exact prompt and answer, the tokens it used and what it cost." For an MCP run, the prompt, the arguments and the result of each tool call sit in one conversation view, in order.

You then find the run by its content. The same page says, "Search finds a run by what the model said, and every log line links back to the step that wrote it." If a user reports a wrong refund amount, search for the phrase the model used and open the run that contains it.

### Redact before you store

Arguments and results carry emails, tokens and customer data. [mcpd's privacy documentation](https://mcpd.sh/docs/privacy) puts the order plainly: "Redaction runs before the write, because a secret deleted afterwards was still written down."

A keyed hash lets you match the same value across runs without keeping the value.

### The platform-team case, same binary

A platform team gets the same binary with different settings.

According to [mcpd's security documentation](https://mcpd.sh/docs/security), API keys carry six scopes: ingest, read, write, execute, worker and admin.

If your team already runs a backend, forward to it. [The mcpd README](https://github.com/loruorg/mcpd) says, "What it stores it also forwards, as OTLP, to any endpoint you name."

### Where MCP Inspector fits

[The MCP documentation](https://modelcontextprotocol.io/docs/tools/inspector) describes the Inspector as "an interactive developer tool for testing and debugging MCP servers." Use it locally to exercise a server you're building. Production traces add the arguments real models generated for real users, and what those models did next.

## How do you find a tool-call loop or a cost spike in MCP traffic?

You find a tool-call loop or a cost spike by starting at the symptom, opening the one run behind it, and then turning what you saw into a check that fires once.

1. **Find the loop.** Open the session that complained and look for one tool name repeating with near-identical arguments.

 A tool such as `lookup_order`, called again and again because its result never satisfies the model, is the case `no_tool_loop` is built to catch. Filter by its score, open the run, and the repeated spans sit in a row, each with the same arguments.

2. **Trace the latency regression.** Every step sits on one clock, so you can see whether the model step or the tool step grew.

 A wide tool span points at the server or the network, and a wide model span points at prompt size or the provider. Don't compare the server's own timings with client timings from another machine, because the two clocks drift apart.

3. **Trace the cost spike.** Sort runs by cost and open the costliest. Find the tool result with the largest payload: it becomes input tokens on the next model step, and that step carries the cost.

 [mcpd's cost docs](https://mcpd.sh/docs/cost) say cost is priced once at ingest, per step, from a price book, and that an unknown model has no cost, not a cost of zero. A step with no cost means the price book lacks that model, so add it before you trust any total.

4. **Turn each pattern into a check.** Set monitors on `error_rate`, `latency_p95` and `cost`. They latch, fire once, resolve only on a real reading and call your webhook, so point the webhook at a channel someone reads.

 [mcpd's alerting docs](https://mcpd.sh/docs/alerting) add that a latched monitor that stops receiving data stays latched, because silence is not recovery. Keep `no_tool_loop` and `tool_efficiency` running on new runs so a loop shows up as a low score you can open.

The outcome is a path of three clicks: a fired monitor, the exemplar run, the span that explains it. [mcpd's documentation on runs](https://mcpd.sh/docs/runs) shows what each step holds.

## Frequently asked questions

### How do I monitor an MCP server in production?

Point an OpenTelemetry exporter at a collector or at mcpd with `OTEL_EXPORTER_OTLP_ENDPOINT`, record each tool call as a span using `gen_ai.*` attributes, carry `traceparent` across the client-server boundary, and mark `isError` results as errored spans. Then read each run's prompt, arguments and result.

### Does observability add a failure point to the MCP path?

Observability with mcpd adds no failure point, because, as [the mcpd README](https://github.com/loruorg/mcpd) explains, mcpd is a process, not a proxy, and nothing travels through it to reach a model or an MCP server. If mcpd is slow or down, your tool calls are unaffected and your agent keeps working.

### What is the difference between mcpd mcp and instrumenting MCP calls?

According to [mcpd's integrating docs](https://mcpd.sh/docs/integrating), `mcpd mcp` exposes `find_runs`, `get_run` and `run_stats` over stdio so an agent can read traces. Instrumenting MCP calls is the other direction: your spans flow into mcpd.

### Where do MCP server logs go over stdio?

[The MCP transports specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) says a stdio server may write UTF-8 log strings to standard error, and the client may capture, forward or ignore them. Standard output is reserved for valid MCP messages, so never print logs there.

### Can I use existing OpenTelemetry or OpenInference instrumentation?

You can, because [mcpd's OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) state that mcpd reads both `gen_ai.*` and OpenInference `llm.*` attributes. Point your current exporter at mcpd and leave your code as it is.

## Next steps for model context protocol observability

The next steps for model context protocol observability run from one traced run today to scorers and monitors within a month.

- **Today:** Open one run, from your own agent or from `./target/release/mcpd demo`, and read the prompt, the tool arguments and the tool result.
- **This week:** Map every `isError: true` result to an errored span, so failed calls stop passing as healthy. Turn on redaction before storage, so secrets in arguments and results are never written.
- **This month:** Add the `no_tool_loop` scorer and monitors on `error_rate`, `latency_p95` and `cost`, so a loop, a slow tool or a spike in tool-result size reaches you as one alert.

To keep the data on your own machine, build it from source and run the same steps.

## Sources

[Tools - Model Context Protocol specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) · [MCP Server Observability: Logging and Tracing Tool Calls](https://inference.net/content/mcp-server-observability/) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Transports - Model Context Protocol](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [mcpd README](https://github.com/loruorg/mcpd). Every other source is linked where the article cites it.
