# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""bench_load.py times the format terminumd asks tmux for (terminum.md AC-20)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from bench_load import list_panes_format  # noqa: E402


class TheFormat(unittest.TestCase):
    def test_is_titles_rs_with_every_field(self):
        fmt = list_panes_format()
        self.assertTrue(fmt.startswith("#{session_name}\t#{window_index}\t"), fmt[:40])
        self.assertIn("\t#{?#{m:*--agent-name *,", fmt)
        self.assertTrue(fmt.endswith("\t#{window_activity}"), fmt[-40:])

    def test_a_source_that_no_longer_says_it_so_raises(self):
        with self.assertRaises(AttributeError):
            list_panes_format("pub const OTHER: &str = \"\";")


if __name__ == "__main__":
    unittest.main()
