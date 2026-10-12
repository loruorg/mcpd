"""The bounded queue and the log throttle (contract B3)."""

import unittest

from mcpd.queue import BoundedQueue, Throttle


class BoundedQueueTest(unittest.TestCase):
    def test_holds_everything_below_the_bound(self):
        q = BoundedQueue(8)
        for i in range(5):
            self.assertTrue(q.put("span", i))
        self.assertEqual(len(q), 5)
        self.assertEqual(q.dropped, 0)

    def test_drops_the_oldest_when_full_and_counts_it(self):
        q = BoundedQueue(3)
        for i in range(10):
            accepted = q.put("span", i)
            self.assertEqual(accepted, i < 3)
        self.assertEqual(len(q), 3, "the queue must never grow past its bound")
        self.assertEqual(q.dropped, 7)
        # The survivors are the newest, because the oldest were the ones dropped.
        self.assertEqual([payload for _, payload in q.drain(10)], [7, 8, 9])

    def test_a_keyed_entry_is_updated_in_place(self):
        q = BoundedQueue(4)
        q.put("span", "started", key=("t", "s"))
        q.put("span", "ended", key=("t", "s"))
        self.assertEqual(len(q), 1)
        self.assertEqual(q.drain(10), [("span", "ended")])

    def test_a_keyed_entry_keeps_its_place_in_the_queue(self):
        q = BoundedQueue(4)
        q.put("span", "first", key="a")
        q.put("span", "second", key="b")
        q.put("span", "first-updated", key="a")
        self.assertEqual([p for _, p in q.drain(10)], ["first-updated", "second"])

    def test_a_dropped_key_can_be_queued_again(self):
        q = BoundedQueue(1)
        q.put("span", "a", key="a")
        q.put("span", "b", key="b")  # evicts "a"
        q.put("span", "a2", key="a")  # evicts "b"; "a" must not still be indexed
        self.assertEqual(q.drain(10), [("span", "a2")])

    def test_drain_takes_at_most_the_limit_oldest_first(self):
        q = BoundedQueue(100)
        for i in range(10):
            q.put("log", i)
        self.assertEqual([p for _, p in q.drain(4)], [0, 1, 2, 3])
        self.assertEqual(len(q), 6)

    def test_a_bound_below_one_is_refused(self):
        with self.assertRaises(ValueError):
            BoundedQueue(0)


class ThrottleTest(unittest.TestCase):
    def test_allows_one_per_interval_and_counts_the_rest(self):
        t = Throttle(interval=3600)
        self.assertEqual(t.check(), 0, "the first call always passes")
        for _ in range(5):
            self.assertIsNone(t.check())
        t._last = 0.0  # pretend the interval elapsed
        self.assertEqual(t.check(), 5, "the suppressed count comes back with the next line")


if __name__ == "__main__":
    unittest.main()
