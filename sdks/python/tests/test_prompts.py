"""Prompts (contract section 5.1) and the trace and span filters of section 5."""

import json
import logging
import threading
import unittest
import urllib.parse

from mcpd import (
    Mcpd,
    Prompt,
    PromptKind,
    PromptRenderError,
    SpanKind,
    render_prompt,
)
from mcpd.transport import TransportError, encode_params

from tests.test_client import FakeTransport


def prompt_body(name="greet", version=3, labels=("production",), template="Hi {{name}}"):
    return {"project": "tests", "name": name, "version": version, "kind": "text",
            "created_at": "2026-08-01T12:00:00Z", "variables": ["name"],
            "labels": list(labels), "template": template}


class PromptTransport(FakeTransport):
    """Answers reads from a script, and remembers every request it was given.

    `answers` maps a label (or `None` for no selector) to a body or an
    exception, so one test can make `production` 404 and the latest succeed.
    """

    def __init__(self, answers=None):
        super().__init__()
        self.answers = dict(answers or {})
        self.reads = []
        self.writes = []
        self.lock_reads = threading.Lock()

    def get(self, path, params=None):
        params = dict(params or {})
        with self.lock_reads:
            self.reads.append((path, params))
        selector = params.get("label")
        if selector is None:
            selector = params.get("version")
        answer = self.answers.get(selector, TransportError("not found", status=404))
        if isinstance(answer, BaseException):
            raise answer
        return answer

    def post(self, path, value, hold=0.0):
        from mcpd.transport import encode

        self.writes.append(("POST", path, encode(value)))
        return prompt_body(name=value.name, version=1, labels=value.labels or [])

    def request(self, method, path, body=None, headers=None, hold=0.0):
        self.writes.append((method, path, json.loads(body) if body else None))
        return prompt_body(version=json.loads(body)["version"])


class Clock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


def client(transport):
    c = Mcpd("http://mcpd.test", project="tests", transport=transport, flush_interval=0.02)
    c._prompts.clock = Clock()
    return c


def text(template, version=1):
    return Prompt(project="tests", name="p", version=version, kind=PromptKind.TEXT,
                  created_at=None, template=template)  # type: ignore[arg-type]


class GetPromptTest(unittest.TestCase):
    def test_the_default_label_falls_back_to_the_latest_version_on_a_404(self):
        t = PromptTransport({None: prompt_body(version=7, labels=())})
        c = client(t)
        p = c.get_prompt("greet")
        self.assertIsInstance(p, Prompt)
        self.assertEqual(p.version, 7)
        self.assertIs(p.kind, PromptKind.TEXT)
        self.assertEqual(t.reads, [
            ("/v1/api/prompts/greet", {"project": "tests", "label": "production"}),
            ("/v1/api/prompts/greet", {"project": "tests"}),
        ])
        c.shutdown()

    def test_the_default_asks_for_production_and_stops_there_when_it_exists(self):
        t = PromptTransport({"production": prompt_body(version=3)})
        c = client(t)
        self.assertEqual(c.get_prompt("greet").version, 3)
        self.assertEqual(len(t.reads), 1)
        c.shutdown()

    def test_a_label_or_a_version_is_sent_as_given_and_names_are_quoted(self):
        t = PromptTransport({"staging": prompt_body(), 2: prompt_body(version=2)})
        c = client(t)
        c.get_prompt("team/greet", label="staging")
        c.get_prompt("team/greet", version=2)
        self.assertEqual(t.reads[0], ("/v1/api/prompts/team%2Fgreet",
                                      {"project": "tests", "label": "staging", "version": None}))
        self.assertEqual(encode_params(t.reads[1][1]), "project=tests&version=2")
        c.shutdown()

    def test_a_404_for_an_explicit_label_is_not_retried_without_it(self):
        t = PromptTransport({None: prompt_body()})
        c = client(t)
        with self.assertRaises(TransportError) as caught:
            c.get_prompt("greet", label="staging")
        self.assertEqual(caught.exception.status, 404)
        self.assertEqual(len(t.reads), 1)
        c.shutdown()

    def test_both_selectors_are_refused_before_the_wire(self):
        t = PromptTransport({"production": prompt_body()})
        c = client(t)
        with self.assertRaises(ValueError):
            c.get_prompt("greet", label="production", version=1)
        self.assertEqual(t.reads, [])
        c.shutdown()

    def test_a_fresh_copy_is_served_with_no_request(self):
        t = PromptTransport({"production": prompt_body()})
        c = client(t)
        first = c.get_prompt("greet")
        c._prompts.clock.now += 59
        self.assertIs(c.get_prompt("greet"), first)
        self.assertEqual(len(t.reads), 1)
        c._prompts.clock.now += 2
        c.get_prompt("greet")
        self.assertEqual(len(t.reads), 2, "an expired copy is fetched again")
        c.shutdown()

    def test_the_selector_is_part_of_the_cache_key(self):
        t = PromptTransport({"production": prompt_body(version=3),
                             "staging": prompt_body(version=4)})
        c = client(t)
        self.assertEqual(c.get_prompt("greet").version, 3)
        self.assertEqual(c.get_prompt("greet", label="staging").version, 4)
        c.shutdown()

    def test_a_stale_copy_is_served_when_the_refetch_fails_and_reported_once(self):
        t = PromptTransport({"production": prompt_body(version=3)})
        c = client(t)
        first = c.get_prompt("greet")
        t.answers["production"] = TransportError("unreachable", retryable=True)
        c._prompts.clock.now += 3600
        with self.assertLogs("mcpd", logging.WARNING) as logs:
            self.assertIs(c.get_prompt("greet"), first)
            self.assertIs(c.get_prompt("greet"), first)
        self.assertEqual(len(logs.records), 1, "an outage warns once, not once per call")
        self.assertEqual(len(t.reads), 3)
        c.shutdown()

    def test_cache_ttl_zero_fetches_every_time_and_still_serves_stale(self):
        t = PromptTransport({"production": prompt_body(version=3)})
        c = client(t)
        c.get_prompt("greet", cache_ttl=0)
        c.get_prompt("greet", cache_ttl=0)
        self.assertEqual(len(t.reads), 2)
        t.answers["production"] = TransportError("down", status=503)
        with self.assertLogs("mcpd", logging.WARNING):
            self.assertEqual(c.get_prompt("greet", cache_ttl=0).version, 3)
        c.shutdown()

    def test_a_fallback_is_served_when_nothing_was_fetched_and_never_cached(self):
        t = PromptTransport({"production": TransportError("down", status=503)})
        c = client(t)
        p = c.get_prompt("greet", fallback="Hello {{ who }}, about {{topic}} and {{who}}")
        self.assertEqual((p.name, p.version, p.labels, p.project), ("greet", 0, [], "tests"))
        self.assertIs(p.kind, PromptKind.TEXT)
        self.assertEqual(p.variables, ["who", "topic"])
        chat = c.get_prompt("greet", fallback=[{"role": "user", "content": "Hi {{name}}"}])
        self.assertIs(chat.kind, PromptKind.CHAT)
        self.assertEqual(chat.variables, ["name"])
        self.assertEqual(len(c._prompts), 0, "a fallback is never cached")
        t.answers["production"] = prompt_body(version=5)
        self.assertEqual(c.get_prompt("greet", fallback="x").version, 5)
        c.shutdown()

    def test_with_no_copy_and_no_fallback_the_fetch_error_is_raised(self):
        t = PromptTransport({"production": TransportError("down", status=500)})
        c = client(t)
        with self.assertRaises(TransportError):
            c.get_prompt("greet")
        c.shutdown()

    def test_the_cache_is_bounded_and_evicts_the_oldest(self):
        t = PromptTransport({"production": prompt_body()})
        c = client(t)
        for i in range(300):
            c.get_prompt(f"p{i}")
        self.assertEqual(len(c._prompts), 256)
        self.assertIsNone(c._prompts.get(("tests", "p0", ("default",))))
        self.assertIsNotNone(c._prompts.get(("tests", "p299", ("default",))))
        c.shutdown()

    def test_a_disabled_client_still_fetches(self):
        t = PromptTransport({"production": prompt_body()})
        c = Mcpd("http://mcpd.test", project="tests", transport=t, enabled=False)
        self.assertEqual(c.get_prompt("greet").version, 3)
        c.shutdown()


class WritePromptTest(unittest.TestCase):
    def test_create_prompt_posts_the_request_in_the_project(self):
        t = PromptTransport()
        c = client(t)
        p = c.create_prompt("greet", template="Hi {{name}}", labels=["production"], note="first")
        self.assertEqual(p.version, 1)
        method, path, body = t.writes[0]
        self.assertEqual((method, path), ("POST", "/v1/api/prompts?project=tests"))
        self.assertEqual(body, {"name": "greet", "template": "Hi {{name}}", "note": "first",
                                "labels": ["production"]})
        c.create_prompt("chat", kind="chat", messages=[{"role": "user", "content": "x"}])
        self.assertEqual(t.writes[1][2]["kind"], "chat")
        c.shutdown()

    def test_set_prompt_label_puts_the_version_and_quotes_the_path(self):
        t = PromptTransport()
        c = client(t)
        p = c.set_prompt_label("team/greet", "production", 2)
        self.assertEqual(p.version, 2)
        self.assertEqual(t.writes[0], (
            "PUT", "/v1/api/prompts/team%2Fgreet/labels/production?project=tests", {"version": 2}))
        c.shutdown()


class RenderTest(unittest.TestCase):
    def test_whitespace_inside_the_braces_is_allowed(self):
        self.assertEqual(render_prompt(text("{{city}} / {{ city }} / {{\tcity\n}}"),
                                       {"city": "Leeds"}), "Leeds / Leeds / Leeds")

    def test_a_placeholder_that_is_not_a_name_is_left_as_written(self):
        tpl = 'JSON {{"a": 1}} {{9x}} {{ }} {{a-b}} {{é}} {{ok.v_2}} and {{open'
        self.assertEqual(render_prompt(text(tpl), {"ok.v_2": "yes"}),
                         'JSON {{"a": 1}} {{9x}} {{ }} {{a-b}} {{é}} yes and {{open')

    def test_substitution_is_one_pass(self):
        self.assertEqual(render_prompt(text("{{a}} {{b}}"), {"a": "{{b}}", "b": "B"}), "{{b}} B")

    def test_non_string_values_are_written_plainly(self):
        self.assertEqual(render_prompt(text("{{n}} {{f}}"), {"n": 3, "f": 1.5}), "3 1.5")

    def test_every_missing_variable_is_named_in_order_of_first_use(self):
        with self.assertRaises(PromptRenderError) as caught:
            render_prompt(text("{{b}} {{a}} {{b}} {{c}}"), {"a": 1, "extra": 2})
        self.assertEqual(caught.exception.missing, ["b", "c"])
        self.assertIn("b, c", str(caught.exception))
        self.assertIsInstance(caught.exception, ValueError)

    def test_chat_content_is_rendered_and_other_fields_kept(self):
        messages = [
            {"role": "system", "content": "You help {{user}}.", "name": "{{user}}"},
            {"role": "user", "content": [{"type": "text", "text": "{{user}}"}]},
        ]
        p = Prompt(project="tests", name="c", version=1, kind=PromptKind.CHAT,
                   created_at=None, messages=messages)  # type: ignore[arg-type]
        out = render_prompt(p, {"user": "Ana"})
        self.assertEqual(out, [
            {"role": "system", "content": "You help Ana.", "name": "{{user}}"},
            {"role": "user", "content": [{"type": "text", "text": "{{user}}"}]},
        ])
        self.assertEqual(messages[0]["content"], "You help {{user}}.", "the prompt is not edited")
        with self.assertRaises(PromptRenderError) as caught:
            render_prompt(p, {})
        self.assertEqual(caught.exception.missing, ["user"])


class SetPromptTest(unittest.TestCase):
    def attributes_after(self, prompt):
        c = Mcpd("http://mcpd.test", project="tests", transport=FakeTransport(),
                 flush_interval=0.02)
        with c.span("call", kind=SpanKind.MODEL_GENERATION) as span:
            self.assertIs(span.set_prompt(prompt), span)
        c.flush(2)
        final = [s for s in c._transport.records_of() if s.end_time is not None][0]
        c.shutdown()
        return final.attributes or {}

    def test_both_attributes_are_set(self):
        attrs = self.attributes_after(text("x", version=4))
        self.assertEqual(attrs["mcpd.prompt.name"], "p")
        self.assertEqual(attrs["mcpd.prompt.version"], 4)

    def test_a_fallback_sets_the_name_only(self):
        attrs = self.attributes_after(text("x", version=0))
        self.assertEqual(attrs, {"mcpd.prompt.name": "p"})

    def test_a_bad_prompt_sets_nothing_and_never_raises(self):
        class Hostile:
            @property
            def name(self):
                raise RuntimeError("boom")

        for bad in (None, "greet", 42, {"name": 3}, {"name": "x", "version": "2"},
                    {"name": "x", "version": True}, Hostile()):
            self.assertEqual(self.attributes_after(bad), {}, repr(bad))


class QueryFilterTest(unittest.TestCase):
    """`list_traces` and `list_spans` send the documented filters and nothing else."""

    class Recording(FakeTransport):
        def __init__(self):
            super().__init__()
            self.reads = []

        def get(self, path, params=None):
            self.reads.append((path, encode_params(params or {})))
            return {"page": {"has_more": False}}

    def test_list_traces_sends_the_new_filters(self):
        t = self.Recording()
        c = client(t)
        c.list_traces(max_duration_ms=900, max_cost=0.5, min_tokens=1000, has_score="rel",
                      score_above=0.8, has_feedback=False, tags=["paid", "eu"])
        path, query = t.reads[0]
        self.assertEqual(path, "/v1/api/traces")
        self.assertEqual(urllib.parse.parse_qsl(query), [
            ("project", "tests"), ("tags", "paid,eu"), ("max_duration_ms", "900"),
            ("max_cost", "0.5"), ("min_tokens", "1000"), ("has_score", "rel"),
            ("score_above", "0.8"), ("has_feedback", "false"),
        ])
        c.list_traces(has_feedback=True)
        self.assertEqual(t.reads[1][1], "project=tests&has_feedback=true")
        c.shutdown()

    def test_list_spans_sends_the_prompt_filters(self):
        t = self.Recording()
        c = client(t)
        c.list_spans(prompt="team/greet", prompt_version=3)
        self.assertEqual(t.reads[0], ("/v1/api/spans",
                                      "project=tests&prompt=team%2Fgreet&prompt_version=3"))
        c.list_spans()
        self.assertEqual(t.reads[1][1], "project=tests", "an unset filter is not sent")
        c.shutdown()


if __name__ == "__main__":
    unittest.main()
