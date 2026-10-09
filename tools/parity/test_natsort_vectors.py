"""The file of name orders that the Rust tests read must be what the real natsort gives."""

import unittest

import natsort_vectors


class NatsortVectorTests(unittest.TestCase):
    def test_the_committed_file_is_what_natsort_gives(self):
        self.assertEqual(natsort_vectors.VECTORS.read_text(encoding="utf-8"), natsort_vectors.render())

    def test_the_cases_that_started_it_come_out_as_kcc_orders_them(self):
        groups = list(natsort_vectors.groups())
        self.assertIn(["p01.png", "p01 (2).png", "p01-2.png", "p01_b.png"], groups)
        self.assertIn(["1.png", "1.5.png", "1.10.png", "2.png", "10.png"], groups)
        self.assertIn(["1.png", "２.png", "3.png", "４.png", "１０.png"], groups)

    def test_no_two_names_of_a_group_have_the_same_key(self):
        key = natsort_vectors.key()
        for group in natsort_vectors.groups():
            self.assertEqual(len({key(name) for name in group}), len(group), group)


if __name__ == "__main__":
    unittest.main()
