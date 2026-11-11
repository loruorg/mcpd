---
description: Wrong answer or looping tool? Start MCP agent debugging by finding the one bad run, then read the prompt, tool arguments and result at each step. Learn how.
faq:
  - answer: To debug an MCP agent, find the one run that went wrong and read the prompt, completion, tool arguments and tool result at each step. Check the tool call for a wrong tool, bad arguments, an ignored error or a loop, and export OpenTelemetry traces so every run is recorded.
    question: How do I debug an MCP agent instead of an MCP server?
  - answer: Your server passes in the Inspector because the Inspector lets you invoke tools yourself, so it never shows the model choosing between them. Open the model-call span and read the tool list and descriptions in the prompt beside the tool call in the completion to see why the model picked that tool.
    question: Why does my server pass in the Inspector but the agent calls the wrong tool?
  - answer: "Logging over notifications/message is deprecated as of protocol version 2026-07-28, so new work shouldn't depend on it. Instead, the MCP debugging guide points to stderr for the stdio transport and OpenTelemetry for all transports, so choose one of those for any logging you add to a new server."
    question: Does MCP logging over notifications/message still work?
  - answer: "In a default mcpd configuration, prompts stay on your machine, and mcpd doesn't check for updates, report usage or send telemetry. Redaction (drop, mask, partial or keyed hash) runs before storage on every signal, so a secret is never written down. The mcpd security documentation covers the details."
    question: Can I debug an MCP agent without sending prompts to a third party?
image: /blog/mcp-agent-debugging/featured-63814db8a6be.webp
image_alt: "Illustration for MCP agent debugging: Find the one run that failed"
publishDate: "2026-11-11"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/mcp-agent-debugging#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\",\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q193231\",\"@type\":\"Thing\",\"description\":\"computer program used to test and debug other programs\",\"name\":\"Debugger\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Debugger\",\"https://www.wikidata.org/wiki/Q193231\",\"https://www.google.com/search?kgmid=/m/0dbw4\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q19841877\",\"@type\":\"Thing\",\"description\":\"source code editor developed by Microsoft\",\"name\":\"Visual Studio Code\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Visual_Studio_Code\",\"https://www.wikidata.org/wiki/Q19841877\",\"https://www.google.com/search?kgmid=/m/0134xwrk\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q634179\",\"@type\":\"Thing\",\"description\":\"intentional stopping or pausing place in a program, used for debugging\",\"name\":\"Breakpoint\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Breakpoint\",\"https://www.wikidata.org/wiki/Q634179\",\"https://www.google.com/search?kgmid=/m/03dcln\"]}],\"datePublished\":\"2026-11-11\",\"description\":\"Wrong answer or looping tool? Start MCP agent debugging by finding the one bad run, then read the prompt, tool arguments and result at each step. Learn how.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"To debug an MCP agent, find the one run that went wrong and read the prompt, completion, tool arguments and tool result at each step. Check the tool call for a wrong tool, bad arguments, an ignored error or a loop, and export OpenTelemetry traces so every run is recorded.\"},\"name\":\"How do I debug an MCP agent instead of an MCP server?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Your server passes in the Inspector because the Inspector lets you invoke tools yourself, so it never shows the model choosing between them. Open the model-call span and read the tool list and descriptions in the prompt beside the tool call in the completion to see why the model picked that tool.\"},\"name\":\"Why does my server pass in the Inspector but the agent calls the wrong tool?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Logging over notifications/message is deprecated as of protocol version 2026-07-28, so new work shouldn't depend on it. Instead, the MCP debugging guide points to stderr for the stdio transport and OpenTelemetry for all transports, so choose one of those for any logging you add to a new server.\"},\"name\":\"Does MCP logging over notifications/message still work?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"In a default mcpd configuration, prompts stay on your machine, and mcpd doesn't check for updates, report usage or send telemetry. Redaction (drop, mask, partial or keyed hash) runs before storage on every signal, so a secret is never written down. The mcpd security documentation covers the details.\"},\"name\":\"Can I debug an MCP agent without sending prompts to a third party?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q8777\",\"@type\":\"Thing\",\"description\":\"application-layer protocol for transferring web resources\",\"name\":\"HTTP\",\"sameAs\":[\"https://en.wikipedia.org/wiki/HTTP\",\"https://www.wikidata.org/wiki/Q8777\",\"https://www.google.com/search?kgmid=/m/03hgt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\",\"@type\":\"Thing\",\"description\":\"text-based open standard designed for human-readable data interchange\",\"name\":\"JSON\",\"sameAs\":[\"https://en.wikipedia.org/wiki/JSON\",\"https://www.wikidata.org/wiki/Q2063\",\"https://www.google.com/search?kgmid=/m/05cntt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q2283\",\"@type\":\"Organization\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q3206079\",\"@type\":\"Thing\",\"description\":\"software debugger\",\"name\":\"LLDB\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LLDB_(debugger)\",\"https://www.wikidata.org/wiki/Q3206079\",\"https://www.google.com/search?kgmid=/m/0c42380\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q949532\",\"@type\":\"Thing\",\"description\":\"chronological record of computer operations\",\"name\":\"Logging\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Logging_(computing)\",\"https://www.wikidata.org/wiki/Q949532\",\"https://www.google.com/search?kgmid=/m/02qxqnj\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q1192634\",\"@type\":\"Thing\",\"description\":\"currently used hierarchical file system directory associated with a running process in an operating system\",\"name\":\"Working directory\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Working_directory\",\"https://www.wikidata.org/wiki/Q1192634\",\"https://www.google.com/search?kgmid=/m/014_pk\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\",\"@type\":\"Thing\",\"description\":\"programming language, superset of JavaScript that compiles to JavaScript\",\"name\":\"TypeScript\",\"sameAs\":[\"https://en.wikipedia.org/wiki/TypeScript\",\"https://www.wikidata.org/wiki/Q978185\",\"https://www.google.com/search?kgmid=/m/0n50hxv\"]}],\"name\":\"MCP agent debugging: Find the one run that failed\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/16p4cC0cn_PHxsSS_NwRwCHDlV0ujCih.webp\"},\"url\":\"https://mcpd.sh/blog/mcp-agent-debugging\"},{\"@id\":\"https://mcpd.sh/blog/mcp-agent-debugging#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\"},{\"@id\":\"https://www.wikidata.org/wiki/Q193231\"},{\"@id\":\"https://www.wikidata.org/wiki/Q19841877\"},{\"@id\":\"https://www.wikidata.org/wiki/Q634179\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Debugging - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Model Context Protocol (MCP) - LLDB\",\"url\":\"https://lldb.llvm.org/use/mcp.html\"},{\"@type\":\"CreativeWork\",\"name\":\"DebugMCP (MCP Server) - Empowering AI Agents with Operational Debugging Capabilities\",\"url\":\"https://github.com/microsoft/DebugMCP\"},{\"@type\":\"CreativeWork\",\"name\":\"Tools - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/server/tools\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol Exporter\",\"url\":\"https://opentelemetry.io/docs/specs/otel/protocol/exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry GenAI attributes registry\",\"url\":\"https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd cost documentation\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Testing · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"HTTP API · mcpd\",\"url\":\"https://mcpd.sh/docs/reference/api\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd security documentation\",\"url\":\"https://mcpd.sh/docs/security\"}],\"dateModified\":\"2026-11-11\",\"datePublished\":\"2026-11-11\",\"description\":\"Wrong answer or looping tool? Start MCP agent debugging by finding the one bad run, then read the prompt, tool arguments and result at each step. Learn how.\",\"headline\":\"MCP agent debugging: Find the one run that failed\",\"image\":\"https://api.birchseek.com/m/16p4cC0cn_PHxsSS_NwRwCHDlV0ujCih.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/mcp-agent-debugging#webpage\"},\"keywords\":[\"mcp agent debugging\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/mcp-agent-debugging#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q8777\"},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q2283\"},{\"@id\":\"https://www.wikidata.org/wiki/Q3206079\"},{\"@id\":\"https://www.wikidata.org/wiki/Q949532\"},{\"@id\":\"https://www.wikidata.org/wiki/Q1192634\"},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2670},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - mcp
  - agent debugging
  - opentelemetry
  - tool calls
  - llm observability
  - tracing
title: "MCP agent debugging: Find the one run that failed"
---

**MCP agent debugging means finding the one run that went wrong, then reading the prompt, completion, tool arguments and tool result at each step, starting with the tool call: wrong tool, wrong arguments, ignored error or loop. Breakpoints and the [MCP Inspector](https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging) cover code and servers, not model behaviour. Export OpenTelemetry traces so [mcpd records every run](https://mcpd.sh).**

Your agent answers correctly on your laptop. In production it gives a wrong answer, the bill jumps, or one tool gets called again and again, and you have a complaint with no obvious run to open.

A debugger shows variable values, but in MCP agent debugging the evidence is the prompt the model received and the completion it returned. You'll be able to go from a complaint to a single run, diagnose its tool call, send traces out with OpenTelemetry, and catch a repeat automatically.

> **Key Takeaways**
> - A stdio MCP server writes logs to stderr and keeps stdout for protocol messages, as the [MCP debugging guide](https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging) sets out.
> - A failed call is either a protocol error or a result with `isError` set to true. Check which before blaming the model.
> - Compare input tokens and price step by step to locate a cost spike, since tool descriptions and results add tokens to every call.
> - A scorer for repeated tool calls and a cost monitor catch a repeat failure unattended.

## How do you start MCP agent debugging from one bad run?

MCP agent debugging starts from a production complaint and ends at one recorded run, which you find by searching what the model said instead of attaching a debugger to a live process.

![Illustration for How do you start MCP agent debugging from one bad run?](/blog/mcp-agent-debugging/image-1-9ac2c01e5a86.webp)

Code debuggers work the other way. [LLDB's MCP support](https://lldb.llvm.org/use/mcp.html) lets an agent "set breakpoints, inspect memory, step through code." A debugger reads a program while it runs. mcpd reads a run after it's finished.

The vocabulary follows mcpd's [concepts](https://mcpd.sh/docs/concepts). A trace is the stored record of one run of your agent. A span is one timed operation inside it, and a step is a span you open to read.

1. Write the symptom as a phrase: a sentence from the user's complaint, a tool name or an error string. A wrong answer gives you words the model probably said.
2. Paste the phrase into mcpd search. It matches the text of prompts and completions, and [search finds a run by what the model said](https://mcpd.sh), so a label like the user's ID isn't needed. If nothing matches, shorten the phrase to its most distinctive three or four words.
3. Narrow by error, cost or latency. For a cost spike, sort the matches by cost. For a looping tool call, keep runs with an unusually high step count. Stop when one run is left.
4. Open the waterfall. Every span sits on one clock, so repeated tool spans and long gaps are visible at once. Note which step first goes wrong, not which step ends the run.
5. Open the failing step and read the exact prompt, the completion, the tool arguments and the tool result, with cost and latency beside them. The cause is usually an earlier tool result that sits in this step's prompt, so don't stop at the final answer.

The mcpd home page describes what a step holds: "Open any step to read the exact prompt and answer, the tokens it used and what it cost."

A complaint that your support agent promised a refund that never arrived shows the method. Search for "your refund has been issued", keep the match with an error span, and open its waterfall. The refund tool step has failed.

In the next step's prompt, the tool result says the call failed, yet the completion tells the user it succeeded. That prompt and completion pair is the bug, and no breakpoint would have shown it.

## How do you debug an MCP agent's wrong or looping tool calls?

A wrong or looping tool call falls into one of four patterns, and you identify each from two spans: the model-call span (the prompt with the tool list, the completion with the tool call) and the tool-execution span (arguments and result).

The [MCP tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) says "Tools use two error reporting mechanisms". A protocol error is a JSON-RPC error, for cases such as an unknown tool. A tool execution error is reported inside the result with `isError` set to true.

The second kind arrives as an ordinary result, so the model reads it as data in its next prompt.

The specification lists "Invalid arguments" among protocol errors, so a malformed call can fail before the tool runs at all. A call that passes validation but hits a failing API comes back as a tool execution error. Check which of the two you have before you blame the model.

1. Wrong tool chosen: the answer comes from the wrong source, for example a web search where a database lookup was needed. Open the model-call span and read the tool list in the prompt against the tool name in the completion. The likely cause is two tools with overlapping descriptions, or the right tool missing from the list.
2. Wrong arguments: the tool returns nothing useful, or rejects the call. Open the tool-execution span and compare the arguments with the tool's input schema, field by field. A vague parameter description is the likely cause, so the model invents a field, drops a required one or sends the wrong type.
3. A tool error the model ignored: the run ends with a confident answer that doesn't match the data. Open the tool-execution span and look for a result flagged with `isError`, then open the next model-call span to see whether the completion noticed it. The likely cause is error text that gives no next step.
4. A loop of repeated identical calls: the same tool runs with the same arguments across many steps while the cost climbs. Open the waterfall, compare the arguments of consecutive tool spans, then read the completion after each result. The likely cause is a result that never changes what the model knows, such as an empty list, together with no step limit.

In pattern 3, put the failed result and the next completion side by side. If the model received the failure and answered as if it had succeeded, the fix belongs in the prompt or the error text, not in the tool.

## How do you export OpenTelemetry traces for MCP agent debugging?

You get OpenTelemetry traces out of an MCP agent by pointing the OTLP exporter in your existing instrumentation at an endpoint, using two environment variables. The [MCP debugging guide](https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging) recommends this route for logs: it lists "OpenTelemetry (all transports)" next to stderr for stdio servers.

1. Set the two variables in the same environment that starts the agent process, because the exporter reads them at startup.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

 The [OpenTelemetry exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) defines `OTEL_EXPORTER_OTLP_PROTOCOL` with three values: `grpc`, `http/protobuf` and `http/json`. mcpd ingests OTLP over HTTP (JSON and protobuf) and gRPC, so any of them works.

2. Add your project key before the first run, so the first trace lands in your project. The mcpd [OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) show how to pass it.

3. Run the agent once and open the run. Each model call and each tool execution arrives as a span carrying attributes from the [OpenTelemetry generative AI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/):

 - `gen_ai.request.model`: the model the step asked for.

mcpd reads both `gen_ai.*` and the OpenInference `llm.*` conventions, so instrumentation you already run works unchanged. The SDKs are optional.

The [mcpd README](https://github.com/loruorg/mcpd) states that "nothing travels through mcpd to reach a model", so if mcpd is down, the agent keeps working and the traces for that period are missing.

To self-host, build from source at `github.com/loruorg/mcpd` with `make build`, then run `./target/release/mcpd demo` and open `http://localhost:4317`. Point the same two variables at that address.

To skip the build, [start free on the hosted version](https://mcpd.sh/app) and use `https://api.mcpd.sh` as the endpoint.

## How do you debug an MCP agent that cannot connect or start?

An MCP agent that can't connect or start fails before any tool runs, usually because of stray output on stdout, a wrong working directory or environment, or a protocol version the other side doesn't speak.

Diagnostics go to stderr, which the [debugging guide](https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging) names as the stdio log route.

If your server prints a banner at startup, the client reads that line from stdout as protocol traffic and the stream breaks. The fix is one line:

```typescript
// Breaks the stdio connection: stdout carries only protocol messages
console.log("server ready");

// Safe: stderr is free for diagnostics
console.error("server ready");
```

In MCP agent debugging, working directory and environment variables cause the quieter failures. A stdio server runs as a child process of the client, so it gets the directory and environment the client gives it.

A relative path or an API key that exists only in your shell works when you start the server by hand and fails under the client.

Version mismatches won't show up on their own, so check them directly. Compare the protocol version each side runs, because the guide's logging advice changes with it.

The MCP Inspector is the first stop. The guide describes it as an "interactive, transport-agnostic testing UI" that connects to stdio or Streamable HTTP servers, invokes tools, prompts and resources, and watches the notification stream.

A recorded trace picks up where the Inspector's session ends: with a real model choosing the tools. To send server telemetry on any transport, [mcpd's OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) cover the exporter setup.

## Where do MCP tools add tokens and cost to each step?

MCP tools add tokens in two places: each tool's description travels in the prompt of every model call, and each tool's result comes back as input on the next call. A long tool list or a chatty tool therefore raises input tokens on every later step, not only the step that used the tool.

The count lives on the model-call span. The OpenTelemetry GenAI semantic conventions define [`gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`](https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/) for it. Input covers the whole prompt, including the tool list and earlier tool results. Output covers the completion, including any tool call the model asks for.

mcpd reads those attributes and prices each step once at ingest, from a [price book of per-million-token rates](https://mcpd.sh/docs/cost). The book handles cached tokens, long-prompt tiers and service tiers, and you can supply your own.

An unknown model has no cost, not a cost of zero, so an empty cost cell means the book lacks that model.

To read cost for one run:

1. Open the run in the waterfall. Each step shows its cost and latency on the same clock.
2. Follow the input tokens down the model-call steps and find the step where they jump.
3. Open that step's prompt and compare it with the step before. A new tool result pasted in, or a tool list at the top, accounts for the growth.

The interface also totals cost per run, per model and per project, so you can go from a spike on the project view to the one run behind it. The [concepts page](https://mcpd.sh/docs/concepts) defines the span and step vocabulary used here.

Once you've found the cause, you can shorten a tool description, trim the tool list, or make the tool return less.

## How do you catch a repeated agent failure without watching?

You catch a repeated agent failure without watching by scoring every run with deterministic scorers and attaching a monitor that fires once when the readings cross a threshold.

1. Score each run with the built-in scorers. None of them calls a model, so the same run always gets the same score.
2. Know what a score holds. Each score declares its scale, and mcpd keeps scores apart from human feedback instead of averaging the two.
3. Create a monitor on a metric. mcpd evaluates [monitors every 60 s](https://mcpd.sh/docs/alerting) on `error_rate`, `latency_p95`, `cost`, `score_mean`, `trace_count` and step-count divergence. A `score_mean` monitor has nothing to read until the scorers have scored some runs, so attach them first, then add your webhook URL.
4. Rely on the latch. A monitor fires once when it crosses its threshold, stays latched, and resolves only on a real reading back inside the limit, and it calls your webhook. An hour with no traffic is silence, not recovery.
5. Turn the failed run into a test. Open the run the webhook pointed to and save it as a dataset case. Then run both agent versions against it and compare them, as described in mcpd's [testing documentation](https://mcpd.sh/docs/testing).

A monitor that fires on every reading soon gets muted, so one message per incident is the version people still read. The result is one webhook call per incident and a looping run that now fails a test before it ships.

## Can a coding agent read traces to debug another agent?

A coding agent can read traces to debug another agent through `mcpd mcp`, which lets it query recorded runs over MCP.

The [DebugMCP repository](https://github.com/microsoft/DebugMCP) describes letting "AI agents debug your code inside VS Code - set breakpoints, step through execution, inspect variables, and evaluate expressions." That fits code you can run under a debugger.

A production agent that gave a wrong answer yesterday has already finished, so for MCP agent debugging you need the record of what the model received and returned.

mcpd's [guide to integrating mcpd with your agents](https://mcpd.sh/docs/integrating) places the MCP route beside the HTTP API and the SDKs. `mcpd mcp` speaks MCP over stdio with three tools:

- `find_runs` finds runs that match what you describe.
- `get_run` returns one run, with its steps.
- `run_stats` returns statistics across runs.

For a complaint that a refund agent kept calling the same tool, your coding or support agent calls `find_runs` for the complaint and opens the match with `get_run`. It reads the repeated tool call and its result, then summarises the loop in plain language from the recorded run.

The server obeys the same key scopes as the API. [API keys have six scopes](https://mcpd.sh/docs/reference/api): ingest, read, write, execute, worker and admin.

An agent holding a read-scoped key can find, read and summarise runs but can't change anything, so you can hand the investigation to an assistant without giving it write access.

## Frequently asked questions

### How do I debug an MCP agent instead of an MCP server?

An agent goes wrong in its decisions, so MCP agent debugging means you search for the run behind the complaint and open the step where the tool call first goes wrong. A server can be checked on its own by invoking its tools directly.

### Why does my server pass in the Inspector but the agent calls the wrong tool?

Open the model-call span and read the tool list and descriptions in the prompt beside the tool call in the completion.

### Does MCP logging over notifications/message still work?

Logging over `notifications/message` is deprecated as of protocol version 2026-07-28, so new work shouldn't depend on it. The MCP debugging guide points to stderr for the stdio transport and OpenTelemetry for all transports.

### Can I debug an MCP agent without sending prompts to a third party?

In a default mcpd configuration, prompts stay on your machine, and mcpd doesn't check for updates, report usage or send telemetry, as the [mcpd security documentation](https://mcpd.sh/docs/security) states.

## What should you do next in MCP agent debugging?

Your next step is one recorded run of your own agent, open in front of you.

- Today, point an OpenTelemetry exporter at `https://api.mcpd.sh`, or build from source at github.com/loruorg/mcpd, and open the first trace.
- This week, search for the run behind your last complaint. Read the failing prompt and completion, then the tool arguments and result on the step before it.

[Start free at mcpd.sh/app](https://mcpd.sh/app), or build from source if you'd rather keep every prompt on your own machine.

## Sources

[Debugging - Model Context Protocol](https://modelcontextprotocol.io/docs/2026-07-28/tools/debugging) · [mcpd · The unopinionated agent observatory](https://mcpd.sh) · [Model Context Protocol (MCP) - LLDB](https://lldb.llvm.org/use/mcp.html) · [Tools - Model Context Protocol](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) · [mcpd cost documentation](https://mcpd.sh/docs/cost) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [Alerting · mcpd](https://mcpd.sh/docs/alerting) · [DebugMCP (MCP Server) - Empowering AI Agents with…](https://github.com/microsoft/DebugMCP). Every other source is linked where the article cites it.
