import re
import tempfile
import unittest
from pathlib import Path

import sync_wiki as w

ROOT = Path(__file__).resolve().parent.parent


class RewriteTests(unittest.TestCase):
    def test_links_between_docs_become_wiki_pages(self):
        self.assertEqual(w.rewrite_target("API.md", "docs/README.md"), "API")
        self.assertEqual(w.rewrite_target("DATA_MODEL.md#settings", "docs/ARCHITECTURE.md"), "Data-Model#settings")
        self.assertEqual(w.rewrite_target("HEALTH.md", "docs/README.md"), "Health-Checks")

    def test_the_root_readme_is_the_overview_page(self):
        self.assertEqual(w.rewrite_target("../README.md", "docs/README.md"), "Overview")
        self.assertEqual(w.rewrite_target("docs/API.md", "README.md"), "API")
        self.assertEqual(w.rewrite_target("docs/README.md", "README.md"), "Home")

    def test_other_repo_files_become_absolute_github_links(self):
        self.assertEqual(
            w.rewrite_target("../src/spa.rs", "docs/ARCHITECTURE.md"),
            f"{w.REPO_URL}/blob/main/src/spa.rs",
        )
        self.assertEqual(
            w.rewrite_target("../grafana/", "docs/OBSERVABILITY.md"),
            f"{w.REPO_URL}/tree/main/grafana",
        )
        self.assertEqual(
            w.rewrite_target("../grafana/dash.json#x", "docs/OBSERVABILITY.md"),
            f"{w.REPO_URL}/blob/main/grafana/dash.json#x",
        )

    def test_screenshots_become_wiki_relative_images(self):
        # from a doc in docs/ and from the root README
        self.assertEqual(w.rewrite_target("images/search.png", "docs/USING.md"), "images/search.png")
        self.assertEqual(w.rewrite_target("docs/images/search.png", "README.md"), "images/search.png")
        self.assertEqual(
            w.rewrite_links("![x](images/a.png) and [y](API.md)", "docs/USING.md"),
            "![x](images/a.png) and [y](API)",
        )

    def test_external_links_and_in_page_anchors_are_untouched(self):
        for t in ("https://example.com/a.md", "http://x", "mailto:a@b.c", "#section"):
            self.assertEqual(w.rewrite_target(t, "docs/API.md"), t)

    def test_rewrites_inline_links_and_images_but_not_fenced_code(self):
        text = "See [the API](API.md) and ![d](../img/x.png).\n```\n[not a link](API.md)\n```\nAfter [it](API.md#x)."
        out = w.rewrite_links(text, "docs/README.md")
        self.assertIn("[the API](API)", out)
        self.assertIn(f"![d]({w.REPO_URL}/blob/main/img/x.png)", out)
        self.assertIn("[not a link](API.md)", out)  # inside the fence: left alone
        self.assertIn("[it](API#x)", out)

    def test_pages_say_where_they_came_from(self):
        t = w.page_text("# T\r\nbody\r\n", "docs/API.md")
        self.assertTrue(t.startswith("# T\nbody\n"))
        self.assertIn("generated from [`docs/API.md`]", t)
        self.assertNotIn("\r", t)


class RepoTests(unittest.TestCase):
    """Checks against the real docs, so a new doc or a broken link can't slip by."""

    def test_every_doc_has_a_wiki_page(self):
        docs = {f"docs/{p.name}" for p in (ROOT / "docs").glob("*.md")} | {"README.md"}
        self.assertEqual(docs - set(w.PAGES), set(), "add new docs to PAGES in scripts/sync_wiki.py")
        for src in w.PAGES:
            self.assertTrue((ROOT / src).exists(), src)

    def test_every_screenshot_a_doc_uses_exists(self):
        used = set()
        for src in w.PAGES:
            for m in w.LINK.finditer((ROOT / src).read_text(encoding="utf-8")):
                t = m.group(2).partition("#")[0]
                base = src.rsplit("/", 1)[0] if "/" in src else ""
                path = f"{base}/{t}" if base else t
                if "images/" in path and not t.startswith("http"):
                    used.add(path.replace("docs/docs/", "docs/"))
        missing = [u for u in used if not (ROOT / u).exists()]
        self.assertEqual(missing, [])

    def test_the_sidebar_lists_exactly_the_generated_pages(self):
        self.assertEqual(sorted(w.SIDEBAR_ORDER), sorted(set(w.PAGES.values())))

    def test_every_relative_doc_link_resolves_to_a_page_or_a_real_repo_path(self):
        import posixpath

        bad = []
        for src in w.PAGES:
            in_fence = False
            for n, line in enumerate((ROOT / src).read_text(encoding="utf-8").split("\n"), 1):
                if w.FENCE.match(line):
                    in_fence = not in_fence
                    continue
                if in_fence:
                    continue
                for m in w.LINK.finditer(line):
                    t = m.group(2)
                    if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", t) or t.startswith("#"):
                        continue
                    path = t.partition("#")[0]
                    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(src), path))
                    if resolved not in w.PAGES and not (ROOT / resolved).exists():
                        bad.append(f"{src}:{n} -> {t}")
        self.assertEqual(bad, [])

    def test_sync_writes_every_page_and_a_sidebar(self):
        with tempfile.TemporaryDirectory() as d:
            written = w.sync(ROOT, Path(d))
            pages = {f"{n}.md" for n in w.PAGES.values()} | {"_Sidebar.md"}
            images = {f"images/{p.name}" for p in (ROOT / w.IMAGES_DIR).glob("*") if p.suffix in w.IMAGE_SUFFIXES}
            self.assertEqual(set(written), pages | images)
            for name in images:
                self.assertTrue((Path(d) / name).is_file(), name)
            home = (Path(d) / "Home.md").read_text(encoding="utf-8")
            self.assertIn("](ARCHITECTURE.md)", (ROOT / "docs/README.md").read_text(encoding="utf-8"))
            self.assertIn("](Architecture)", home)
            self.assertNotIn("](ARCHITECTURE.md)", home)


    def test_files_without_an_extension_link_as_blob_and_folders_as_tree(self):
        # LICENSE is a file in the repository root; examples/ and grafana/ are folders.
        self.assertEqual(w.rewrite_target("../LICENSE", "docs/README.md"), f"{w.REPO_URL}/blob/main/LICENSE")
        self.assertEqual(w.rewrite_target("../examples/smb", "docs/README.md"), f"{w.REPO_URL}/tree/main/examples/smb")


if __name__ == "__main__":
    unittest.main()
