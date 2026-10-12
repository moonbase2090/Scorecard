#!/usr/bin/env python3
"""Regression checks for published-release installation links."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("check_docs", Path(__file__).with_name("check-docs.py"))
check_docs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_docs)


class PublishedReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        check_docs.ROOT = Path(self.directory.name)
        (check_docs.ROOT / "docs").mkdir()
        (check_docs.ROOT / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.7"\n')
        (check_docs.ROOT / "README.md").write_text(
            "Published release [v0.1.6](https://github.com/moonbase2090/Scorecard/releases/tag/v0.1.6).\n"
        )

    def test_unpublished_workspace_bump_keeps_published_install_links(self):
        (check_docs.ROOT / "docs/install.md").write_text(
            "VERSION=v0.1.6\nVER=0.1.6\n"
            "https://github.com/moonbase2090/Scorecard/releases/download/v0.1.6/SHA256SUMS\n"
            "sc-v0.1.6-aarch64-apple-darwin.tar.gz\n"
            "moonbase2090/Scorecard/action@v0.1.6\n"
        )
        self.assertEqual(check_docs.check_versions(), [])

    def test_each_mismatched_install_pin_is_rejected(self):
        for pin in [
            "releases/download/v0.1.7/SHA256SUMS",
            "releases/tag/v0.1.7",
            "sc-v0.1.7-aarch64-apple-darwin.tar.gz",
            "VERSION=v0.1.7",
            "VER=0.1.7",
            "Scorecard/action@v0.1.7",
        ]:
            with self.subTest(pin=pin):
                (check_docs.ROOT / "docs/install.md").write_text(pin + "\n")
                self.assertEqual(
                    check_docs.check_versions(),
                    ["docs/install.md: v0.1.7 should be published release v0.1.6"],
                )

    def test_missing_published_release_is_an_error(self):
        (check_docs.ROOT / "README.md").write_text("# Scorecard\n")
        self.assertEqual(
            check_docs.check_versions(),
            ["README.md: declare the published release with `Published release [vX.Y.Z](URL)`"],
        )


if __name__ == "__main__":
    unittest.main()
