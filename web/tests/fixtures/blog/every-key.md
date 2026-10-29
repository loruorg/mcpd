---
title: "Every front matter key: a test post"
description: "A post that sets every front matter key the blog accepts, so a build can prove each one renders."
publishDate: 2026-10-01
updatedDate: "2026-10-15"
tags: ["testing", "tracing"]
faq:
  - question: "Does the FAQ render?"
    answer: "Yes. Each question is a heading and each answer a paragraph."
  - question: "Can an answer have two paragraphs?"
    answer: "Yes.\n\nA blank line starts the second one."
author: "Test Author"
image: "/og/default.png"
image_alt: "The mcpd share card"
schema_json: '{"@context":"https://schema.org","@type":"BlogPosting","headline":"Every front matter key: a test post","x-fixture":"schema-json-verbatim"}'
slug: "a-slug-the-file-name-overrides"
canonical: "https://example.com/elsewhere"
unknown_key: [1, 2, 3]
---

The first paragraph comes before any heading, the way an introduction does.

## A second-level heading

A paragraph with a [link to the docs](/docs/quickstart), **strong text**, *emphasis* and `inline code`. A claim with a citation.[^1]

### A third-level heading

- An item
- An item with a nested list:
  - A nested item
- A last item

1. First step
2. Second step
3. Third step

> A quotation, set off from the text around it.

| Setting | Default | What it does | Where to set it | Since | Notes |
| --- | --- | --- | --- | --- | --- |
| `--sample` | `1.0` | Keeps a share of traces | Flag or config file | 0.1 | A wide table scrolls inside its own frame |
| `--port` | `4317` | The port mcpd listens on | Flag | 0.1 | Second row |

```ts
const answer: number = 42
console.log(answer)
```

![A site image](/og/default.png)

Another claim with a second citation.[^2]

---

The last paragraph, after a rule.

[^1]: The first source, [example.com](https://example.com/source-one).
[^2]: The second source.
