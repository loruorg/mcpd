"""Model calls (contract 3.1) and in-process redaction (contract 2.1.1)."""

import re
import time
import unittest
from types import SimpleNamespace

from mcpd import Mcpd, RecordingSink, SpanKind, records, usage_from_anthropic, usage_from_openai

OPENAI_CHAT = {
    "id": "chatcmpl-1",
    "object": "chat.completion",
    "model": "gpt-4o-2024-08-06",
    "service_tier": "flex",
    "choices": [{"index": 0, "finish_reason": "stop",
                 "message": {"role": "assistant", "content": "hi"}}],
    "usage": {
        "prompt_tokens": 1000,
        "completion_tokens": 200,
        "total_tokens": 1200,
        "prompt_tokens_details": {"cached_tokens": 600, "audio_tokens": 0},
        "completion_tokens_details": {"reasoning_tokens": 50},
    },
}

ANTHROPIC_MESSAGE = {
    "id": "msg_1",
    "type": "message",
    "model": "claude-sonnet-4-5",
    "stop_reason": "end_turn",
    "usage": {
        "input_tokens": 100,
        "cache_read_input_tokens": 900,
        "cache_creation_input_tokens": 50,
        "cache_creation": {"ephemeral_5m_input_tokens": 20, "ephemeral_1h_input_tokens": 30},
        "output_tokens": 40,
        "service_tier": "batch",
    },
}


class FakeModel:
    """Stands in for a provider SDK's pydantic model: read through `model_dump`."""

    def __init__(self, data):
        self._data = data

    def model_dump(self):
        return self._data


def client(**kwargs):
    kwargs.setdefault("sink", RecordingSink())
    kwargs.setdefault("flush_interval", 10)
    kwargs.setdefault("project", "test")
    return Mcpd("http://daemon.test", **kwargs)


def last(spans, name):
    return [s for s in spans if s.name == name][-1]


class UsageMappingTest(unittest.TestCase):
    def test_openai_usage_splits_the_gross_prompt_into_text_and_cache(self):
        self.assertEqual(usage_from_openai(OPENAI_CHAT), records.TokenUsage(
            input_tokens=1000, output_tokens=200, total_tokens=1200,
            input_details=records.InputTokenDetails(text=400, cache_read=600, audio=0),
            output_details=records.OutputTokenDetails(reasoning=50),
        ))
        responses = {
            "object": "response",
            "usage": {"input_tokens": 10, "output_tokens": 5,
                      "input_tokens_details": {"cached_tokens": 4}},
        }
        self.assertEqual(usage_from_openai(responses), records.TokenUsage(
            input_tokens=10, output_tokens=5, total_tokens=15,
            input_details=records.InputTokenDetails(text=6, cache_read=4),
        ))

    def test_anthropic_usage_input_is_net_of_the_cache(self):
        self.assertEqual(usage_from_anthropic(ANTHROPIC_MESSAGE), records.TokenUsage(
            input_tokens=1050, output_tokens=40, total_tokens=1090,
            input_details=records.InputTokenDetails(text=100, cache_read=900, cache_write=50,
                                                    cache_write_5m=20, cache_write_1h=30),
        ))

    def test_provider_objects_are_read_as_well_as_dicts(self):
        self.assertEqual(usage_from_openai(FakeModel(OPENAI_CHAT)), usage_from_openai(OPENAI_CHAT))
        usage = SimpleNamespace(input_tokens=100, cache_read_input_tokens=900,
                                cache_creation_input_tokens=50,
                                cache_creation=SimpleNamespace(ephemeral_5m_input_tokens=20,
                                                               ephemeral_1h_input_tokens=30),
                                output_tokens=40)
        message = SimpleNamespace(type="message", usage=usage)
        self.assertEqual(usage_from_anthropic(message), usage_from_anthropic(ANTHROPIC_MESSAGE))

    def test_an_unknown_shape_reads_nothing_and_never_raises(self):
        self.assertIsNone(usage_from_openai(None))
        self.assertIsNone(usage_from_anthropic({"usage": {"input_tokens": "many"}}))
        self.assertIsNone(usage_from_openai({"usage": {"prompt_tokens": True}}))
        c = client()
        span = c.trace("run")
        self.assertIs(span.record_response(42).record_response({"usage": "x"}), span)
        c.shutdown()


class RecordResponseTest(unittest.TestCase):
    def test_records_model_usage_id_finish_reasons_and_tier_never_the_text(self):
        c = client()
        c.trace("a", kind=SpanKind.MODEL_GENERATION).record_response(OPENAI_CHAT).end()
        c.trace("b", kind=SpanKind.MODEL_GENERATION).record_response(
            FakeModel(ANTHROPIC_MESSAGE), "aws.bedrock").end()
        self.assertTrue(c.flush(2))
        a, b = last(c._sink.spans, "a"), last(c._sink.spans, "b")
        self.assertEqual(a.model, "gpt-4o-2024-08-06")
        self.assertEqual(a.provider, "openai")
        self.assertIsNone(a.output)
        self.assertEqual(a.attributes, {
            "gen_ai.response.id": "chatcmpl-1",
            "gen_ai.response.finish_reasons": ["stop"],
            "gen_ai.response.service_tier": "flex",
        })
        self.assertEqual(a.usage.input_tokens, 1000)
        self.assertEqual(b.provider, "aws.bedrock")
        self.assertEqual(b.attributes["gen_ai.response.service_tier"], "batch")
        self.assertEqual(b.attributes["gen_ai.response.finish_reasons"], ["end_turn"])
        self.assertEqual(b.usage.input_tokens, 1050)
        c.shutdown()

    def test_records_the_first_token_once_and_a_guardrail_outcome(self):
        c = client()
        s = c.trace("stream")
        s.mark_first_token()
        time.sleep(0.005)
        s.mark_first_token()
        s.set_guardrail(True, "pii in output").end()
        self.assertTrue(c.flush(2))
        attrs = last(c._sink.spans, "stream").attributes
        self.assertIsInstance(attrs["mcpd.time_to_first_token_ms"], float)
        self.assertLess(attrs["mcpd.time_to_first_token_ms"], 5)
        self.assertIs(attrs["mcpd.guardrail.tripped"], True)
        self.assertEqual(attrs["mcpd.guardrail.reason"], "pii in output")
        c.shutdown()


class RedactTest(unittest.TestCase):
    def test_rewrites_payloads_before_queueing_and_drops_what_it_raises_on(self):
        def redact(value):
            if isinstance(value, str) and "boom" in value:
                raise ValueError("redactor broke")
            if isinstance(value, dict):
                return {k: re.sub(r"\d{4}-\d{4}", "****", v) if isinstance(v, str) else v
                        for k, v in value.items()}
            return value

        c = client(redact=redact)
        given = {"card": "1234-5678"}
        span = c.trace("run", input=given)
        span.set_output("boom").end()
        c.log("info", "paid", {"card": "1234-5678"})
        self.assertTrue(c.flush(2))
        rec = last(c._sink.spans, "run")
        self.assertEqual(rec.input, {"card": "****"})
        self.assertIsNone(rec.output)
        self.assertEqual(c._sink.logs[0].fields, {"card": "****"})
        self.assertEqual(c.stats().redaction_failures, 1)
        # The caller's own value is untouched.
        self.assertEqual(given, {"card": "1234-5678"})
        c.shutdown()

    def test_an_unusable_mapping_result_fails_closed(self):
        c = client(redact=lambda value: "nope")
        c.log("info", "paid", {"card": "1234-5678"})
        self.assertTrue(c.flush(2))
        self.assertIsNone(c._sink.logs[0].fields)
        self.assertEqual(c.stats().redaction_failures, 1)
        c.shutdown()

    def test_payloads_capture_dropped_never_reach_redact(self):
        seen = []
        c = client(redact=lambda value: seen.append(value) or value, capture_payloads=False)
        c.trace("run", input="secret").end()
        self.assertTrue(c.flush(2))
        self.assertNotIn("secret", seen)
        c.shutdown()


if __name__ == "__main__":
    unittest.main()
