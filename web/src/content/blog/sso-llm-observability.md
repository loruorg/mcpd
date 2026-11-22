---
description: What is SSO for LLM observability? People sign in via OpenID Connect with org roles, and SCIM removes leavers. Learn what to check in a tool and start free.
faq:
  - answer: "You need SSO once more than a handful of people can open traces, because traces hold prompts and completions. A shared API key can't tell you which person used it, while a login through your identity provider ties each session to a named account and a role. A solo operator can start without it."
    question: Do I need SSO for LLM observability?
  - answer: "SSO does not replace API keys in LLM observability. People sign in with SSO, while agents and pipelines use API keys scoped to ingest, read, write, execute, worker or admin. Each key's scope limits what the agent holding it can do."
    question: Does SSO replace API keys?
  - answer: mcpd has no enterprise licence, so SSO does not sit behind one. Accounts, OIDC SSO, SCIM, quotas and an audit log are all configuration, switched on with the --signup flag on the hosted service or the --accounts flag on your own cluster.
    question: Does mcpd charge for SSO?
  - answer: "You can use GitHub as the identity provider through an OAuth app. Google, Microsoft Entra ID, Okta and Auth0 connect as OpenID Connect providers, and every login, whichever provider it comes from, lands in the same four organisation roles: viewer, member, admin or owner."
    question: Can I use GitHub as the identity provider?
  - answer: "SCIM adds joiner and leaver automation: your identity provider creates and deactivates accounts over HTTP, so a leaver loses access without anyone editing the trace store. RFC 7644 defines the protocol to reduce the cost and complexity of user management operations."
    question: What does SCIM add on top of SSO?
image: /blog/sso-llm-observability/featured-9446b5ea41af.webp
image_alt: "Illustration for SSO for LLM observability: Roles, SCIM, audit log"
publishDate: "2026-11-22"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/sso-llm-observability#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]}],\"datePublished\":\"2026-11-22\",\"description\":\"What is SSO for LLM observability? People sign in via OpenID Connect with org roles, and SCIM removes leavers. Learn what to check in a tool and start free.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You need SSO once more than a handful of people can open traces, because traces hold prompts and completions. A shared API key can't tell you which person used it, while a login through your identity provider ties each session to a named account and a role. A solo operator can start without it.\"},\"name\":\"Do I need SSO for LLM observability?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"SSO does not replace API keys in LLM observability. People sign in with SSO, while agents and pipelines use API keys scoped to ingest, read, write, execute, worker or admin. Each key's scope limits what the agent holding it can do.\"},\"name\":\"Does SSO replace API keys?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd has no enterprise licence, so SSO does not sit behind one. Accounts, OIDC SSO, SCIM, quotas and an audit log are all configuration, switched on with the --signup flag on the hosted service or the --accounts flag on your own cluster.\"},\"name\":\"Does mcpd charge for SSO?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can use GitHub as the identity provider through an OAuth app. Google, Microsoft Entra ID, Okta and Auth0 connect as OpenID Connect providers, and every login, whichever provider it comes from, lands in the same four organisation roles: viewer, member, admin or owner.\"},\"name\":\"Can I use GitHub as the identity provider?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"SCIM adds joiner and leaver automation: your identity provider creates and deactivates accounts over HTTP, so a leaver loses access without anyone editing the trace store. RFC 7644 defines the protocol to reduce the cost and complexity of user management operations.\"},\"name\":\"What does SCIM add on top of SSO?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q132393290\",\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q117246174\",\"@type\":\"Thing\",\"description\":\"artificial intelligence model capable of generating content in response to a prompt\",\"name\":\"Generative AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Generative_AI\",\"https://www.wikidata.org/wiki/Q117246174\",\"https://www.google.com/search?kgmid=/g/11khkg2rwf\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\",\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q568494\",\"@type\":\"Thing\",\"description\":\"property of access control whereby a single login is used to gain access to any of several related systems\",\"name\":\"Single sign-on\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Single_sign-on\",\"https://www.wikidata.org/wiki/Q568494\",\"https://www.google.com/search?kgmid=/m/03qqn0\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q19599791\",\"@type\":\"Thing\",\"description\":\"Application Programming Interface for user provisioning\",\"name\":\"System for Cross-domain Identity Management\",\"sameAs\":[\"https://en.wikipedia.org/wiki/System_for_Cross-domain_Identity_Management\",\"https://www.wikidata.org/wiki/Q19599791\",\"https://www.google.com/search?kgmid=/m/012gbhvw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q1047174\",\"@type\":\"Thing\",\"description\":\"chronological record keeping track of a sequence of activities for security purposes\",\"name\":\"Audit trail\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Audit_trail\",\"https://www.wikidata.org/wiki/Q1047174\",\"https://www.google.com/search?kgmid=/m/0b4tm\"]}],\"name\":\"SSO for LLM observability: Roles, SCIM, audit log\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/tV4qTP23B-yGz09LRJkRi0nRzqHwE1n0.webp\"},\"url\":\"https://mcpd.sh/blog/sso-llm-observability\"},{\"@id\":\"https://mcpd.sh/blog/sso-llm-observability#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Accounts · mcpd\",\"url\":\"https://mcpd.sh/docs/accounts\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"RFC 7644: System for Cross-domain Identity Management: Protocol\",\"url\":\"https://www.rfc-editor.org/rfc/rfc7644\"},{\"@type\":\"CreativeWork\",\"name\":\"RFC 7643: System for Cross-domain Identity Management: Core Schema\",\"url\":\"https://www.rfc-editor.org/rfc/rfc7643\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenID Connect | Google Identity\",\"url\":\"https://developers.google.com/identity/openid-connect/openid-connect\"},{\"@type\":\"CreativeWork\",\"name\":\"OAuth 2.0 and OpenID Connect overview | Okta Developer\",\"url\":\"https://developer.okta.com/docs/concepts/oauth-openid/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenID Connect Core 1.0\",\"url\":\"https://openid.net/specs/openid-connect-core-1_0.html\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenID Connect Discovery 1.0\",\"url\":\"https://openid.net/specs/openid-connect-discovery-1_0.html\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenID Connect (OIDC) on the Microsoft identity platform\",\"url\":\"https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenID Connect Protocol | Auth0 Docs\",\"url\":\"https://auth0.com/docs/authenticate/protocols/openid-connect-protocol\"},{\"@type\":\"CreativeWork\",\"name\":\"Authorizing OAuth apps | GitHub Docs\",\"url\":\"https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"Semantic conventions for generative client AI spans | OpenTelemetry\",\"url\":\"https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/\"},{\"@type\":\"CreativeWork\",\"name\":\"10 Best LLM Observability Tools in 2026, Compared\",\"url\":\"https://langwatch.ai/blog/top-10-llm-observability-tools-complete-guide-for-2026\"},{\"@type\":\"CreativeWork\",\"name\":\"Forwarding · mcpd\",\"url\":\"https://mcpd.sh/docs/forwarding\"}],\"dateModified\":\"2026-11-22\",\"datePublished\":\"2026-11-22\",\"description\":\"What is SSO for LLM observability? People sign in via OpenID Connect with org roles, and SCIM removes leavers. Learn what to check in a tool and start free.\",\"headline\":\"SSO for LLM observability: Roles, SCIM, audit log\",\"image\":\"https://api.birchseek.com/m/tV4qTP23B-yGz09LRJkRi0nRzqHwE1n0.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/sso-llm-observability#webpage\"},\"keywords\":[\"sso for llm observability\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/sso-llm-observability#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q132393290\"},{\"@id\":\"https://www.wikidata.org/wiki/Q117246174\"},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\"},{\"@id\":\"https://www.wikidata.org/wiki/Q568494\"},{\"@id\":\"https://www.wikidata.org/wiki/Q19599791\"},{\"@id\":\"https://www.wikidata.org/wiki/Q1047174\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2714},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - sso
  - llm observability
  - scim
  - openid connect
  - audit log
  - access control
title: "SSO for LLM observability: Roles, SCIM, audit log"
---

**SSO for LLM observability is an OpenID Connect login to the trace store, mapped to organisation roles (viewer, member, admin, owner), with SCIM to add and remove people as they join and leave, and an audit log. In mcpd, accounts, OIDC SSO, SCIM, quotas and the audit log are configuration on [the same mcpd binary](https://mcpd.sh/docs/accounts), hosted or on your own cluster, with no enterprise licence.**

Traces hold customer prompts and completions, and more people want to open them. When an auditor asks who saw what, a shared API key cannot answer, and neither can a local password list. SSO for LLM observability puts access control on the trace store itself, where that payload sits.

A solo operator can skip identity work on day one. A platform team cannot skip SSO for LLM observability. After reading, you can describe the identity layer, separate people from machines, and evaluate a tool on it.

> **Key Takeaways**
> - People sign in with SSO; agents and pipelines use API keys with [six scopes](https://mcpd.sh/docs/security): ingest, read, write, execute, worker, admin.
> - Deprovisioning in your identity provider is the security gain: SCIM ([RFC 7643](https://www.rfc-editor.org/rfc/rfc7643), [RFC 7644](https://www.rfc-editor.org/rfc/rfc7644)) removes a leaver's access without a manual step.
> - [Redaction](https://mcpd.sh/docs/privacy) (drop, mask, partial, keyed hash) runs before storage, so no role can expose what was never written.
> - `--signup` (hosted) and `--accounts` (your cluster) switch on the [same account features](https://mcpd.sh/docs/accounts).

## What does SSO for LLM observability involve?

SSO for LLM observability means people sign in to the trace store through your identity provider using OpenID Connect instead of local passwords. The provider authenticates them, the tool maps them to an organisation and a role such as viewer, member, admin or owner, and that role decides who can open prompts and completions.

![Illustration for What does SSO for LLM observability involve?](/blog/sso-llm-observability/image-1-53e6d7c31abf.webp)

[OpenID Connect Core 1.0](https://openid.net/specs/openid-connect-core-1_0.html) describes the protocol as "a simple identity layer on top of the OAuth 2.0 protocol." In the authorisation-code flow, the tool sends your browser to the provider, which sends back a one-time code.

The tool trades that code for an ID token: a signed token whose claims say who signed in. The spec puts it this way: "The Authorization Code Flow returns an Authorization Code to the Client, which can then exchange it for an ID Token and an Access Token directly."

The tool also has to know where to send the browser. [OpenID Connect Discovery 1.0](https://openid.net/specs/openid-connect-discovery-1_0.html) solves this with a JSON document at `/.well-known/openid-configuration` under the provider's issuer URL. It lets a client "obtain information needed to interact with it, including its OAuth 2.0 endpoint locations."

In practice you register an application with the provider, give it the tool's callback URL, and paste the client ID, client secret and issuer URL into the tool. The tool reads the discovery document and learns the rest itself.

The main identity providers speak OpenID Connect natively:

- [Google's OpenID Connect documentation](https://developers.google.com/identity/openid-connect/openid-connect) says its OAuth 2.0 implementation for authentication "conforms to the OpenID Connect specification."
- [Microsoft's Entra ID documentation](https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc) says "OpenID Connect (OIDC) extends the OAuth 2.0 authorization protocol for use as another authentication protocol."
- [Okta's developer documentation](https://developer.okta.com/docs/concepts/oauth-openid/) describes OpenID Connect as "an authentication standard built on top of OAuth 2.0."
- Auth0 documents its own [OpenID Connect (OIDC)](https://auth0.com/docs/authenticate/protocols/openid-connect-protocol) support.
- GitHub goes through an OAuth app instead. [GitHub's OAuth app documentation](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) says its OAuth implementation supports "the standard authorization code grant type."

Once the ID token is verified, the tool finds or creates the account and places it in an organisation with a role. That is the core of SSO for LLM observability. The role then decides which runs and spans that person can open.

A login session belongs to a person and ends when they sign out or the session expires. An API key belongs to an agent or pipeline and carries scopes, so it never needs a human to sign in.

## SSO logins vs API keys: who gets which access?

A person gets access through an SSO login that becomes a session with an organisation role, while an agent or pipeline gets an API key with a scope.

| | SSO login | API key | No keys set |
| --- | --- | --- | --- |
| Who it is for | A person | An agent or pipeline | A laptop |
| How it is issued | OpenID Connect sign-in at your identity provider | Created in the project | Nothing issued |
| What limits it | Role: viewer, member, admin, owner | The scope on the key | Nothing |
| What revokes it | Removal in your identity provider | Deleting the key | Setting a key |

[mcpd's security docs](https://mcpd.sh/docs/security) say API keys have six scopes (ingest, read, write, execute, worker and admin) and are compared as SHA-256 hashes. The same page says that with no keys set, ingest and reads are open, which is for a laptop only.

Keys outlive people. A key pasted into a script stays after the person who pasted it leaves, so give each pipeline and agent its own key with one scope. People never need a key to read traces: engineers, support staff and auditors sign in, and the role decides what they see.

[mcpd's integration guide](https://mcpd.sh/docs/integrating) says `mcpd mcp` serves three tools, `find_runs`, `get_run` and `run_stats`, over stdio, and it obeys the same key scopes. A coding or support agent that reads traces therefore uses its own read-scoped key.

The first trace needs no identity work. Point a pipeline's OpenTelemetry exporter at the endpoint below and give it the ingest-scoped key from your project. Engineers who read those traces can sign in through SSO later, on the same binary.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

No mcpd package needs installing: any exporter works, as the [OpenTelemetry ingestion guide](https://mcpd.sh/docs/opentelemetry) describes. [Start free at mcpd.sh/app](https://mcpd.sh/app) to get a project and a key with nothing to set up.

## How does SCIM provisioning handle people who join and leave?

SCIM provisioning lets your identity provider create, update and deactivate user accounts in the trace store over HTTP, so the tool learns about joiners and leavers without anyone telling it.

Without it, the failure is quiet. A contractor finishes on Friday, and your identity provider disables their account the same day. The trace store never hears about it, so their account there stays active, with access to customer prompts and completions.

SSO controls who can log in: it checks a person against the identity provider at sign-in. SCIM controls whether the account exists and is active in the tool. With SSO alone, the account record, its role and any sessions already open can outlive the person's employment.

[RFC 7644](https://www.rfc-editor.org/rfc/rfc7644) describes SCIM as "an HTTP-based protocol that makes managing identities in multi-domain scenarios easier to support via a standardized service." [RFC 7643](https://www.rfc-editor.org/rfc/rfc7643), the core schema, defines "a platform-neutral schema and extension model for representing users and groups in JSON format."

Users and Groups are the two resources your identity provider keeps in sync, through the full life of an account:

- Create: the provider sends the new user as JSON when someone is assigned to the app.
- Update: for a changed name or group membership, RFC 7644 says "HTTP PATCH is an OPTIONAL server function that enables clients to update one or more attributes of a SCIM resource".
- Deactivate: the provider sets the user's `active` attribute to false. RFC 7643 defines `active` as "A Boolean value indicating the User's administrative status."

Deprovisioning matters more than provisioning in SSO for LLM observability. A missed creation is noticed quickly, because the new hire can't open a run and files a ticket. A missed deactivation raises no error until an auditor asks who could still read a customer's prompt.

Groups matter for the same reason: moving someone out of a group in the identity provider can remove their organisation role in the tool, so the permission follows the change.

[mcpd's accounts documentation](https://mcpd.sh/docs/accounts) lists SCIM among its account features, alongside organisations, roles, OIDC single sign-on, quotas and an audit log, on the hosted service and on your own cluster.

## How do you answer who saw what in LLM traces?

You answer "who saw what" with three records that work together: the role that limited who could open the run, the audit log of who did, and the redaction mode that decided what was stored in the first place.

For a run whose prompt held a customer's email address, the audit log names who opened it, the role explains why only those people could, and the redaction mode tells you whether the address was ever there to read. Set them up in this order.

1. Set the redaction mode. Choose drop, mask, partial or keyed hash for the sensitive fields.
2. Set retention in days. Exposure lasts as long as the data does, so pick the shortest window that still covers your debugging. When a person asks for their data to go, file an erasure request about that person instead of waiting for retention to expire.
3. Assign roles. Give each person the lowest of viewer, member, admin or owner that fits their job. Treat the right to search as the right to read.
4. Read the audit log. [mcpd's accounts documentation](https://mcpd.sh/docs/accounts) lists an audit log among the account features. Open it for the period in question and match each entry to a person. The log can only answer for the SSO accounts that exist, so keep agents on API keys and people on logins.

With all four in place, SSO for LLM observability can answer the auditor. For any run, you can name the people who could open it, the people who did, and which values of the customer's data were stored.

The [OpenTelemetry GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/) define the attributes that carry this content.

The [GenAI span conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/) mark the message-content attributes, `gen_ai.input.messages` and `gen_ai.output.messages`, as "Opt-In". Those attributes hold the prompt and completion, the data your SSO roles end up protecting.

## Self-hosted or hosted: where does SSO for LLM observability sit?

SSO sits in the same place on both mcpd routes, inside the accounts layer of the daemon, and a flag decides whether mcpd.sh or you run it.

| | mcpd hosted | mcpd self-hosted |
| --- | --- | --- |
| Start route | mcpd.sh/app | Build from source, github.com/loruorg/mcpd |
| Flag | `--signup` | `--accounts` |
| What you stand up | An account and a project | The daemon and its store |
| Where traces are stored | On the hosted service | Your SQLite file, Postgres or ClickHouse |

The two routes differ in where the trace data lives.

On your own cluster, storage is one SQLite file by default, with Postgres or ClickHouse for a team, as [mcpd's security documentation](https://mcpd.sh/docs/security) states.

That choice does not change how SSO for LLM observability works.

[Forwarding as OTLP](https://mcpd.sh/docs/forwarding) to any endpoint means the trace data can move to another tool later, so choosing hosted or self-hosted is a reversible decision.

For other tools, such as Langfuse, LangSmith, LangWatch and Arize Phoenix, read each vendor's own documentation and pricing page for the tier that includes SSO, SCIM and audit logs. [LangWatch's own comparison post](https://langwatch.ai/blog/top-10-llm-observability-tools-complete-guide-for-2026) describes its product as running "self-hosted or hybrid". Whichever tool you evaluate, ask who registers the OIDC application and who runs SCIM.

The hosted version suits a solo operator or small team that wants SSO-capable accounts without running a daemon. Self-hosting suits a platform team whose trace data has to stay in its own network, on its own store.

## What should you check when evaluating SSO in an LLM observability tool?

Evaluate SSO in an LLM observability tool by asking eight questions, each aimed at a failure you would otherwise find in production.

1. Which login protocol does the tool document, and does it work with your identity providers? Match the protocol named in the docs to what your identity provider offers, so the rollout does not stall at security review. mcpd [documents OIDC](https://mcpd.sh/docs/accounts), with Google, Microsoft Entra ID, Okta and Auth0, and GitHub through an OAuth app.
2. Is deprovisioning automatic through SCIM? SCIM is specified in RFC 7643 (schema) and [RFC 7644](https://www.rfc-editor.org/rfc/rfc7644) (protocol). Without it, a contractor who leaves your identity provider keeps a live account in the trace store until someone remembers to delete it.
3. How many roles are there, and what can each one read? A single "user" role means every account can open every prompt. mcpd has four: viewer, member, admin and owner.
4. Do API keys have scopes, so agents never need a human login? A pipeline that runs on a person's credentials breaks the day that person leaves. mcpd has six [key scopes](https://mcpd.sh/docs/security): ingest, read, write, execute, worker and admin.
### Records, storage and tiers

5. Does the audit log record who opened which run? Ask the vendor to show you the entry. Without it, an auditor's question about who saw a customer's prompt has no answer.
6. Is redaction applied before storage, and is retention set in days? A secret removed afterwards was still written down. mcpd [documents redaction before the write](https://mcpd.sh/docs/privacy) (drop, mask, partial or keyed hash) and retention in days.
7. Where do traces live, and can you move them? If the store is fixed, an identity or compliance change means abandoning your history.
8. Which tier holds each feature? Read the vendor's current documentation and pricing page, and write down where SSO, SCIM and the audit log sit, so "Enterprise only" does not surprise you at renewal. mcpd publishes no enterprise licence: accounts run on the hosted service with `--signup` or on your own cluster with `--accounts`.

Run the same eight questions against every tool on your shortlist. Keep the answers in one table, and you have a fair comparison of SSO for LLM observability.

## Frequently asked questions

### Do I need SSO for LLM observability?

You need SSO once more than a handful of people can open traces, because traces hold prompts and completions. A shared API key can't tell you which person used it, while a login through your identity provider ties each session to a named account and a role. A solo operator can start without it.

### Does SSO replace API keys?

SSO doesn't replace them. People sign in with SSO, and agents and pipelines use [API keys](https://mcpd.sh/docs/security) scoped to ingest, read, write, execute, worker or admin. Each key's scope limits what that agent can do.

### Does mcpd charge for SSO?

mcpd has no enterprise licence. [Accounts, OIDC SSO, SCIM](https://mcpd.sh/docs/accounts), quotas and an audit log are configuration, switched on with --signup on the hosted service or --accounts on your own cluster.

### Can I use GitHub as the identity provider?

Yes, through an OAuth app. Google, Microsoft Entra ID, Okta and Auth0 connect as OpenID Connect providers, and every login lands in the same organisation roles: viewer, member, admin or owner.

### What does SCIM add on top of SSO?

SCIM adds joiner and leaver automation: your identity provider creates and deactivates accounts over HTTP, so a leaver loses access without anyone editing the trace store. [RFC 7644](https://www.rfc-editor.org/rfc/rfc7644) defines the protocol to reduce the cost and complexity of user management operations.

## What to do next with SSO for LLM observability

Start small with SSO for LLM observability. Add identity work as more people need to read traces. Nothing here needs a migration.

Today, set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, add the key from your project, run your agent once and read the first trace. The [mcpd quickstart](https://mcpd.sh/docs/quickstart) walks the same path.

This week, list everyone who can open traces. Assign each person a role, then register an OIDC application with your identity provider so people sign in there instead of sharing a key.

This quarter:

- Switch on SCIM so leavers lose access when your provider deactivates them.
- Pick a redaction mode (drop, mask, partial or hash) and a retention period in days.
- Take the auditor's question, "who opened this run?", and check that the audit log answers it.

[Start free at mcpd.sh/app](https://mcpd.sh/app), send one trace before you touch any identity settings, and add SSO when the list of people grows.

## Sources

[Accounts · mcpd](https://mcpd.sh/docs/accounts) · [Security · mcpd](https://mcpd.sh/docs/security) · [RFC 7644: System for Cross-domain Identity Management…](https://www.rfc-editor.org/rfc/rfc7644) · [RFC 7643: System for Cross-domain Identity Management: Core…](https://www.rfc-editor.org/rfc/rfc7643) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [OpenID Connect Core 1.0](https://openid.net/specs/openid-connect-core-1_0.html) · [OpenID Connect Discovery 1.0](https://openid.net/specs/openid-connect-discovery-1_0.html) · [OpenID Connect | Google Identity](https://developers.google.com/identity/openid-connect/openid-connect). Every other source is linked where the article cites it.
