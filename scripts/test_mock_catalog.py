"""Keeps the mock library (dev/mock) honest: the documentation screenshots and the
Wanted / Requests demos depend on its shape."""
import re
import sys
import unittest
from pathlib import Path

MOCK = Path(__file__).resolve().parent.parent / "dev" / "mock"
sys.path.insert(0, str(MOCK))

import catalog  # noqa: E402
import mock_plex  # noqa: E402


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.items = list(catalog.items())

    def test_keys_are_unique_and_every_item_is_in_a_known_section(self):
        keys = [i["rating_key"] for i in self.items]
        self.assertEqual(len(keys), len(set(keys)))
        self.assertTrue({i["section"] for i in self.items} <= {"1", "2", "3"})

    def test_there_are_items_without_subtitles_for_the_wanted_tab(self):
        without = [i for i in self.items if not i["lines"]]
        self.assertEqual({i["title"] for i in without}, catalog.NO_SUBTITLES)
        self.assertEqual(len(without), 6)
        # ...but a download would find something for each of them
        self.assertTrue(all(i["downloadable"] for i in without))

    def test_movies_have_no_show_and_episodes_have_numbers(self):
        for i in self.items:
            if i["kind"] == "movie":
                self.assertIsNone(i["show"])
                self.assertIsNone(i["season"])
            else:
                self.assertTrue(i["show"] and i["season"] and i["episode"])

    def test_lines_repeat_across_shows_so_operators_have_something_to_narrow(self):
        shows_with_soup = {i["show"] for i in self.items if i["lines"] and catalog.SOUP in i["lines"] and i["show"]}
        self.assertGreaterEqual(len(shows_with_soup), 3)

    def test_srt_is_well_formed_and_fits_inside_the_clip(self):
        text = catalog.srt(["one", "two", "three"])
        blocks = [b for b in text.strip().split("\n\n")]
        self.assertEqual(len(blocks), 3)
        last_end = 0
        for n, block in enumerate(blocks, 1):
            idx, timing, line = block.split("\n")
            self.assertEqual(int(idx), n)
            m = re.fullmatch(r"(\d\d):(\d\d):(\d\d),(\d{3}) --> (\d\d):(\d\d):(\d\d),(\d{3})", timing)
            self.assertIsNotNone(m, timing)
            g = list(map(int, m.groups()))
            start = g[0] * 3600 + g[1] * 60 + g[2] + g[3] / 1000
            end = g[4] * 3600 + g[5] * 60 + g[6] + g[7] / 1000
            self.assertLess(start, end)
            self.assertGreaterEqual(start, last_end)
            last_end = end
        self.assertLess(last_end, catalog.CLIP_SECONDS)


class MockPlexTests(unittest.TestCase):
    def test_a_search_offers_five_results_and_only_the_first_ever_arrives(self):
        item = next(catalog.items())
        results = mock_plex.search_results(item, "en")
        self.assertEqual(len(results), 5)
        self.assertIn("/fast-", results[0]["key"])
        self.assertTrue(all("/slow-" in r["key"] for r in results[1:]))
        self.assertTrue(all(r["key"].startswith("/library/metadata/") for r in results))
        self.assertEqual(results[0]["perfectMatch"], "1")

    def test_release_names_look_like_release_names(self):
        ep = next(i for i in catalog.items() if i["kind"] == "episode")
        name = mock_plex.search_results(ep, "en")[0]["title"]
        self.assertRegex(name, r"\.S\d\dE\d\d\.")


if __name__ == "__main__":
    unittest.main()
