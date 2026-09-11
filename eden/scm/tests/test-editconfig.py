# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import Optional

import silenttestrunner
from bindings import configloader
from sapling import config, error, hg as hgmod, ui as uimod
from sapling.rcutil import (
    editconfig,
    formatconfiginclude,
    formatconfigsection,
    formatconfigvalue,
)


class testeditconfig(unittest.TestCase):
    def setUp(self):
        fd, path = tempfile.mkstemp()
        os.close(fd)
        self.path = Path(path)

    def tearDown(self):
        self.path.unlink()

    def testaddconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            "val",
            """
[sec1]
name1 = val
""".lstrip(),
        )

        self.assertedit(
            "sec2",
            "name1",
            "val",
            """
[sec1]
name1 = val

[sec2]
name1 = val
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            "dont\nmessup",
            """
[sec1]
name1 = val
name2 = dont
  messup

[sec2]
name1 = val
""".lstrip(),
        )

    def testeditconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            "foo\nbar",
            """
[sec1]
name1 = foo
  bar
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name1",
            "baz\nqux",
            """
[sec1]
name1 = baz
  qux
""".lstrip(),
        )

    def testdeleteconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            None,
            "",
        )

        self.assertedit(
            "sec1",
            "name1",
            "foo",
            """
[sec1]
name1 = foo
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            "bar\nbaz",
            """
[sec1]
name1 = foo
name2 = bar
  baz
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name1",
            None,
            """
[sec1]
name2 = bar
  baz
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            None,
            """
[sec1]
""".lstrip(),
        )

    def testduplicateentries(self):
        self.path.write_text(
            """
[foo]
bar = one
[foo]
bar = two
""".lstrip()
        )

        self.assertedit(
            "foo",
            "bar",
            "three",
            """
[foo]
bar = one
[foo]
bar = three
""".lstrip(),
        )

    def testformatconfigsectionescapesnewlines(self):
        value = "https://example.invalid/x\n[extensions]\nfoo = python:evil.py\n"
        text = formatconfigsection(" paths ", " default ", value)

        self.assertEqual(
            text,
            "[paths]\n"
            "default = https://example.invalid/x\n"
            "  [extensions]\n"
            "  foo = python:evil.py\n",
        )
        self.assertparseswithoutinjectedextension(text, value.rstrip("\n"))

    def testeditconfigescapesnewlines(self):
        value = "first\n[extensions]\nfoo = python:evil.py\n"
        editconfig(uimod.ui(), self.path, "paths", "default", value)

        self.assertparseswithoutinjectedextension(
            self.path.read_text(),
            value.rstrip("\n"),
        )

    def testrejectblankconfigvaluelines(self):
        for value in ["first\n\nsecond", "first\n  \nsecond"]:
            with self.subTest(value=value):
                with self.assertRaises(error.Abort):
                    formatconfigvalue(value)

    def testrejectinvalidconfigtext(self):
        original = "[section]\noriginal = old\n"
        for section, name, value in [
            ("\nsection", "name", "v"),
            ("bad]section", "name", "v"),
            ("section", "bad=name", "v"),
            ("section", " #badname ", "v"),
            ("section", "name", "bad\rvalue"),
        ]:
            with self.subTest(section=section, name=name, value=value):
                with self.assertRaises(error.Abort):
                    formatconfigsection(section, name, value)
                self.path.write_text(original)
                with self.assertRaises(error.Abort):
                    editconfig(uimod.ui(), self.path, section, name, value)
                self.assertEqual(self.path.read_text(), original)

    def test_editconfig_normalizes_names_before_lookup(self) -> None:
        """Padded names create and update the same item without duplicates."""
        self.assertedit(" paths ", " default ", "first", "[paths]\ndefault = first\n")
        self.assertedit(" paths ", " default ", "second", "[paths]\ndefault = second\n")

    def test_editconfig_preserves_unsectioned_keys(self) -> None:
        """Existing unsectioned items can still be updated and removed."""
        self.path.write_text("name = first\n")
        self.assertedit("", "name", "second", "name = second\n")
        self.assertedit("", "name", None, "\n")

    def testrejectinvalidinclude(self):
        for path in ["", " \t ", "/tmp/config\n[extensions]"]:
            with self.subTest(path=path):
                with self.assertRaises(error.Abort):
                    formatconfiginclude(path)

    def assertparseswithoutinjectedextension(self, text: str, expected: str):
        rustcfg = configloader.config()
        rustcfg.parse(text, source=str(self.path))
        pythoncfg = config.config()
        pythoncfg.parse(str(self.path), text)
        for cfg in [rustcfg, pythoncfg]:
            self.assertEqual(cfg.get("paths", "default"), expected)
            self.assertEqual(cfg.get("extensions", "foo"), None)

    def testeditconfigrejectsinvalidvalues(self):
        self.path.write_text("[sec1]\nname1 = val\n")
        before = self.path.read_text()
        for value in ["bad\0value", "bad\rvalue", "bad\r", "first\n\nsecond"]:
            with self.subTest(value=value):
                with self.assertRaises(error.Abort):
                    editconfig(uimod.ui(), self.path, "sec1", "name1", value)
                self.assertEqual(self.path.read_text(), before)

    def testeditconfignormalizestrailingwhitespace(self):
        # Trailing whitespace is dropped on write, but the config parser
        # strips it on read, so the normalized form reads back identically.
        self.assertedit("sec1", "name1", "val ", "[sec1]\nname1 = val\n")
        self.path.write_text("[sec1]\nname1 = val\n")
        ui = uimod.ui()
        ui.quiet = True
        editconfig(ui, self.path, "sec1", "name2", "a\nb ")
        self.assertEqual(self.path.read_text(), "[sec1]\nname1 = val\nname2 = a\n  b\n")

    def assertedit(self, section: str, name: str, value: Optional[str], expected: str):
        ui = uimod.ui()
        ui.quiet = True
        editconfig(ui, self.path, section, name, value)
        self.assertEqual(
            self.path.read_bytes(), expected.replace("\n", os.linesep).encode()
        )


class testwritehgrc(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.hgrcname = "hgrc"
        self.hgrc = Path(self.tmpdir.name) / self.hgrcname
        identity = SimpleNamespace(configrepofile=lambda: self.hgrcname)
        ui = SimpleNamespace(identity=identity)
        tmpdir = self.tmpdir.name

        def localvfs(path, mode):
            return open(os.path.join(tmpdir, path), mode)

        self.repo = SimpleNamespace(
            wlock=contextlib.nullcontext,
            lock=contextlib.nullcontext,
            localvfs=localvfs,
            ui=ui,
        )

    def tearDown(self):
        self.tmpdir.cleanup()

    def testwritehgrc(self):
        hgmod._writehgrc(self.repo, "/tmp/src", ["/tmp/extra"])
        text = self.hgrc.read_text()
        self.assertIn("default = /tmp/src", text)
        self.assertIn("%include /tmp/extra", text)

    def testwritehgrcvalidatesbeforeopening(self):
        self.hgrc.write_text("[paths]\ndefault = old\n")
        with self.assertRaises(error.Abort):
            hgmod._writehgrc(self.repo, "/tmp/src", ["/tmp/config\n[extensions]"])
        self.assertEqual(self.hgrc.read_text(), "[paths]\ndefault = old\n")


if __name__ == "__main__":
    silenttestrunner.main(__name__)
