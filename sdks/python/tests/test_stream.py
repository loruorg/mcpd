"""`Span.record_stream` (contract 3.2), read through the test client."""

import asyncio
import gc
import types
import unittest

from mcpd import STREAM_MAX_TOOL_CALLS, STREAM_TEXT_BUDGET, SpanStatus
from mcpd.testing import TestClient

OPENAI_CHUNKS = [
    {"id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
     "choices": [{"index": 0, "delta": {"role": "assistant"}}]},
    {"id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
     "choices": [{"index": 0, "delta": {"content": "Hel"}}]},
    {"id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
     "choices": [{"index": 0, "delta": {"content": "lo"}}]},
    {"id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
     "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
    {"id": "chatcmpl-1", "object": "chat.completion.chunk", "model": "gpt-4o",
     "service_tier": "flex", "choices": [],
     "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12,
               "prompt_tokens_details": {"cached_tokens": 4}}},
]

ANTHROPIC_EVENTS = [
    {"type": "message_start",
     "message": {"id": "msg_1", "type": "message", "model": "claude-sonnet-4-5",
                 "usage": {"input_tokens": 20, "cache_read_input_tokens": 80,
                           "output_tokens": 1}}},
    {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
    {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Checking"}},
    {"type": "content_block_stop", "index": 0},
    {"type": "content_block_start", "index": 1,
     "content_block": {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}}},
    {"type": "content_block_delta", "index": 1,
     "delta": {"type": "input_json_delta", "partial_json": '{"id":'}},
    {"type": "content_block_delta", "index": 1,
     "delta": {"type": "input_json_delta", "partial_json": '"A-1"}'}},
    {"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 15}},
    {"type": "message_stop"},
]


class Model:
    """A provider SDK object: read through `model_dump`, as a pydantic model is."""

    def __init__(self, data):
        self._data = data

    def model_dump(self):
        return self._data


def items(chunks, fail_after=None):
    for i, chunk in enumerate(chunks):
        if fail_after is not None and i == fail_after:
            raise ConnectionError("connection reset")
        yield chunk


async def aitems(chunks, fail_after=None):
    for i, chunk in enumerate(chunks):
        if fail_after is not None and i == fail_after:
            raise ConnectionError("connection reset")
        await asyncio.sleep(0)
        yield chunk


async def adrain(stream):
    return [c async for c in stream]


def client(**kwargs):
    kwargs.setdefault("flush_interval", 10.0)
    return TestClient("http://mcpd.test", **kwargs)


class OpenAIStreamTest(unittest.TestCase):
    def check_openai(self, mcpd):
        s = mcpd.recorded_span("call")
        self.assertEqual(s.model, "gpt-4o")
        self.assertEqual(s.provider, "openai")
        self.assertEqual(s.output, "Hello")
        self.assertEqual(s.usage.input_tokens, 10)
        self.assertEqual(s.usage.output_tokens, 2)
        self.assertEqual(s.usage.total_tokens, 12)
        self.assertEqual(s.usage.input_details.text, 6)
        self.assertEqual(s.usage.input_details.cache_read, 4)
        attrs = s.attributes
        self.assertEqual(attrs["gen_ai.response.id"], "chatcmpl-1")
        self.assertEqual(attrs["gen_ai.response.finish_reasons"], ["stop"])
        self.assertEqual(attrs["gen_ai.response.service_tier"], "flex")
        self.assertIsInstance(attrs["mcpd.time_to_first_token_ms"], float)
        self.assertNotIn("mcpd.stream.truncated", attrs)
        self.assertEqual(s.status, SpanStatus.OK)

    def test_sync_stream_records_and_passes_every_chunk(self):
        mcpd = client()
        span = mcpd.trace("call", kind="model_generation")
        sent = OPENAI_CHUNKS + ["not a chunk", None, 7]
        seen = list(span.record_stream(iter(sent)))
        self.assertEqual(seen, sent)
        for a, b in zip(seen, sent):
            self.assertIs(a, b)
        self.check_openai(mcpd)

    def test_usage_from_a_chunk_whose_choices_are_null(self):
        mcpd = client()
        span = mcpd.trace("call")
        usage = {"prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4}
        list(span.record_stream(iter([{"model": "gpt-4o", "choices": None, "usage": usage}])))
        s = mcpd.recorded_span("call")
        self.assertEqual(s.usage.output_tokens, 1)
        self.assertEqual(s.provider, "openai")

    def test_async_stream_records_and_passes_every_chunk(self):
        mcpd = client()
        span = mcpd.trace("call", kind="model_generation")
        sent = OPENAI_CHUNKS + ["not a chunk"]
        seen = asyncio.run(adrain(span.record_stream(aitems(sent))))
        self.assertEqual(seen, sent)
        self.assertIs(seen[1], sent[1])
        self.check_openai(mcpd)

    def test_provider_sdk_objects(self):
        mcpd = client()
        span = mcpd.trace("call")
        chunks = [Model(c) for c in OPENAI_CHUNKS]
        seen = list(span.record_stream(chunks))
        self.assertIs(seen[0], chunks[0])
        self.check_openai(mcpd)

    def test_attribute_objects(self):
        def ns(value):
            if isinstance(value, dict):
                return types.SimpleNamespace(**{k: ns(v) for k, v in value.items()})
            if isinstance(value, list):
                return [ns(v) for v in value]
            return value

        mcpd = client()
        span = mcpd.trace("call")
        list(span.record_stream([ns(c) for c in OPENAI_CHUNKS]))
        self.assertEqual(mcpd.recorded_span("call").output, "Hello")
        self.assertEqual(mcpd.recorded_span("call").model, "gpt-4o")

    def test_tool_call_deltas_joined_by_index(self):
        mcpd = client()
        span = mcpd.trace("call")

        def call(d):
            return {"choices": [{"index": 0, "delta": {"tool_calls": [d]}}]}

        list(span.record_stream([
            call({"index": 0, "id": "call_1", "function": {"name": "lookup", "arguments": '{"a"'}}),
            call({"index": 0, "function": {"arguments": ":1}"}}),
            {"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]},
        ]))
        s = mcpd.recorded_span("call")
        self.assertEqual(s.output, {"content": "", "tool_calls": [
            {"id": "call_1", "name": "lookup", "arguments": '{"a":1}'}]})
        self.assertEqual(s.attributes["gen_ai.response.finish_reasons"], ["tool_calls"])

    def test_explicit_provider_wins(self):
        mcpd = client()
        span = mcpd.trace("call")
        list(span.record_stream(OPENAI_CHUNKS, provider="azure_openai"))
        self.assertEqual(mcpd.recorded_span("call").provider, "azure_openai")


class AnthropicStreamTest(unittest.TestCase):
    def check(self, mcpd):
        s = mcpd.recorded_span("call")
        self.assertEqual(s.model, "claude-sonnet-4-5")
        self.assertEqual(s.provider, "anthropic")
        self.assertEqual(s.output, {"content": "Checking", "tool_calls": [
            {"id": "toolu_1", "name": "lookup", "arguments": '{"id":"A-1"}'}]})
        self.assertEqual(s.usage.input_tokens, 100)
        self.assertEqual(s.usage.output_tokens, 15)
        self.assertEqual(s.attributes["gen_ai.response.id"], "msg_1")
        self.assertEqual(s.attributes["gen_ai.response.finish_reasons"], ["tool_use"])
        self.assertIn("mcpd.time_to_first_token_ms", s.attributes)

    def test_sync(self):
        mcpd = client()
        span = mcpd.trace("call", kind="model_generation")
        self.assertEqual(list(span.record_stream(iter(ANTHROPIC_EVENTS))), ANTHROPIC_EVENTS)
        self.check(mcpd)

    def test_async(self):
        mcpd = client()
        span = mcpd.trace("call", kind="model_generation")
        asyncio.run(adrain(span.record_stream(aitems(ANTHROPIC_EVENTS))))
        self.check(mcpd)


class LifecycleTest(unittest.TestCase):
    def test_first_token_marked_once_at_first_content_chunk(self):
        mcpd = client()
        span = mcpd.trace("call")
        stream = span.record_stream(OPENAI_CHUNKS)
        next(stream)  # role only: no content yet
        self.assertNotIn("mcpd.time_to_first_token_ms", span._attributes or {})
        next(stream)  # the first text
        first = span._attributes["mcpd.time_to_first_token_ms"]
        self.assertEqual(mcpd.spans(), [])
        list(stream)
        self.assertEqual(mcpd.recorded_span("call").attributes["mcpd.time_to_first_token_ms"], first)

    def test_sync_stream_that_raises(self):
        mcpd = client()
        span = mcpd.trace("call")
        with self.assertRaises(ConnectionError) as caught:
            list(span.record_stream(items(OPENAI_CHUNKS, fail_after=3)))
        self.assertEqual(str(caught.exception), "connection reset")
        s = mcpd.recorded_span("call")
        self.assertEqual(s.status, SpanStatus.ERROR)
        self.assertEqual(s.error.message, "connection reset")
        self.assertEqual(s.error.kind, "ConnectionError")
        self.assertEqual(s.output, "Hello")

    def test_async_stream_that_raises(self):
        mcpd = client()
        span = mcpd.trace("call")
        with self.assertRaises(ConnectionError):
            asyncio.run(adrain(span.record_stream(aitems(OPENAI_CHUNKS, fail_after=3))))
        s = mcpd.recorded_span("call")
        self.assertEqual(s.status, SpanStatus.ERROR)
        self.assertEqual(s.error.message, "connection reset")
        self.assertEqual(s.output, "Hello")

    def test_sync_break_ends_the_span(self):
        mcpd = client()
        span = mcpd.trace("call")
        source = items(OPENAI_CHUNKS)
        for chunk in span.record_stream(source):
            if chunk["choices"][0]["delta"].get("content") == "Hel":
                break
        gc.collect()
        s = mcpd.recorded_span("call")
        self.assertEqual(s.output, "Hel")
        self.assertEqual(s.status, SpanStatus.OK)
        # Stopping early also closes the provider's stream.
        self.assertIsNone(source.gi_frame)

    def test_sync_close_ends_the_span_once(self):
        mcpd = client()
        span = mcpd.trace("call")
        stream = span.record_stream(OPENAI_CHUNKS)
        next(stream)
        next(stream)
        stream.close()
        stream.close()
        self.assertEqual(mcpd.recorded_span("call").output, "Hel")
        self.assertEqual(len([s for s in mcpd.recorder.spans if s.end_time is not None]), 1)

    def test_async_break_ends_the_span(self):
        mcpd = client()
        span = mcpd.trace("call")

        async def main():
            stream = span.record_stream(aitems(OPENAI_CHUNKS))
            async for chunk in stream:
                if chunk["choices"][0]["delta"].get("content") == "Hel":
                    break
            await stream.aclose()

        asyncio.run(main())
        s = mcpd.recorded_span("call")
        self.assertEqual(s.output, "Hel")
        self.assertEqual(s.status, SpanStatus.OK)

    def test_async_break_without_aclose_ends_the_span(self):
        mcpd = client()
        span = mcpd.trace("call")

        async def main():
            async for chunk in span.record_stream(aitems(OPENAI_CHUNKS)):
                if chunk["choices"][0]["delta"].get("content") == "Hel":
                    break

        # The event loop finalises the dropped generator before it closes.
        asyncio.run(main())
        self.assertEqual(mcpd.recorded_span("call").output, "Hel")

    def test_cancelled_task_ends_the_span_without_an_error(self):
        mcpd = client()
        span = mcpd.trace("call")

        async def forever():
            yield OPENAI_CHUNKS[1]
            await asyncio.sleep(3600)
            yield OPENAI_CHUNKS[2]

        async def main():
            async def read():
                async for _ in span.record_stream(forever()):
                    pass

            task = asyncio.ensure_future(read())
            await asyncio.sleep(0.01)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task

        asyncio.run(main())
        s = mcpd.recorded_span("call")
        self.assertEqual(s.output, "Hel")
        self.assertEqual(s.status, SpanStatus.OK)

    def test_text_past_the_budget_is_cut(self):
        mcpd = client()
        span = mcpd.trace("call")
        big = "\U0001F600" * (STREAM_TEXT_BUDGET - 1) + "ab"
        list(span.record_stream([{"choices": [{"index": 0, "delta": {"content": big}}]},
                                 {"choices": [{"index": 0, "delta": {"content": "more"}}]}]))
        s = mcpd.recorded_span("call")
        self.assertEqual(len(s.output), STREAM_TEXT_BUDGET)
        self.assertTrue(s.output.endswith("\U0001F600a"))
        self.assertIs(s.attributes["mcpd.stream.truncated"], True)

    def test_budget_is_shared_with_tool_arguments(self):
        mcpd = client()
        span = mcpd.trace("call")
        text = "x" * (STREAM_TEXT_BUDGET - 3)
        list(span.record_stream([
            {"choices": [{"index": 0, "delta": {"content": text}}]},
            {"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "id": "c", "function": {"name": "f", "arguments": "12345"}}]}}]},
        ]))
        s = mcpd.recorded_span("call")
        self.assertEqual(s.output["tool_calls"][0]["arguments"], "123")
        self.assertIs(s.attributes["mcpd.stream.truncated"], True)

    def test_tool_calls_past_the_cap_are_ignored(self):
        mcpd = client()
        span = mcpd.trace("call")
        chunks = [{"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": i, "id": f"c{i}", "function": {"name": "f", "arguments": "{}"}}]}}]}
            for i in range(STREAM_MAX_TOOL_CALLS + 3)]
        list(span.record_stream(chunks))
        s = mcpd.recorded_span("call")
        self.assertEqual(len(s.output["tool_calls"]), STREAM_MAX_TOOL_CALLS)
        self.assertIs(s.attributes["mcpd.stream.truncated"], True)

    def test_capture_off_keeps_the_output_in_the_process(self):
        mcpd = client(capture_payloads=False)
        span = mcpd.trace("call")
        list(span.record_stream(OPENAI_CHUNKS))
        s = mcpd.recorded_span("call")
        self.assertIsNone(s.output)
        self.assertEqual(s.model, "gpt-4o")

    def test_unknown_stream_sets_no_output(self):
        mcpd = client()
        span = mcpd.trace("call")
        self.assertEqual(list(span.record_stream(["a", {"x": 1}])), ["a", {"x": 1}])
        s = mcpd.recorded_span("call")
        self.assertIsNone(s.output)
        self.assertIsNone(s.model)
        self.assertEqual(s.status, SpanStatus.OK)

    def test_a_chunk_that_raises_on_read_is_passed_through(self):
        class Hostile:
            def model_dump(self):
                raise RuntimeError("no")

            @property
            def choices(self):
                raise RuntimeError("no")

        mcpd = client()
        span = mcpd.trace("call")
        chunk = Hostile()
        self.assertEqual(list(span.record_stream([chunk] + OPENAI_CHUNKS))[0], chunk)
        self.assertEqual(mcpd.recorded_span("call").output, "Hello")


if __name__ == "__main__":
    unittest.main()
