# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import tempfile
import unittest

import silenttestrunner
from bindings import configloader


def createConfig():
    return configloader.config()


def normalizeSources(cfg, section, name):
    # normalize [value, (path, start, end, line), name]
    # to use relative path
    sources = cfg.sources(section, name)
    result = []
    for source in sources:
        value, info, name = source
        if info is not None:
            path, start, end, line = info
            path = os.path.basename(path)
            info = (path, start, end, line)
        result.append((value, info, name))
    return result


FIXTURES = {
    "a.rc": rb"""[a]
x=1
y=2
%include b.rc
""",
    "b.rc": rb"""%include b.rc
[b]
z = 3
[a]
%unset y
%include broken.rc
""",
    "broken.rc": rb"%not-implemented",
}


class ConfigParserTests(unittest.TestCase):
    def run(self, result):
        oldpwd = os.getcwd()
        with tempfile.TemporaryDirectory() as t:
            os.chdir(t)
            for name, content in FIXTURES.items():
                with open(name, "wb") as f:
                    f.write(content)
            super().run(result)
            # Needed on Windows to delete the temp dir.
            os.chdir(oldpwd)

    def testReadConfig(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, None, None)
        cfg.parse("[c]\nx=1", "parse")
        cfg.set("d", "y", "2", "set1")
        cfg.set("d", "x", None, "set2")
        self.assertEqual(cfg.sections(), ["a", "b", "c", "d"])
        self.assertEqual(cfg.get("a", "x"), "1")
        self.assertEqual(cfg.get("a", "y"), None)
        self.assertEqual(cfg.get("b", "z"), "3")
        self.assertEqual(cfg.get("c", "x"), "1")
        self.assertEqual(cfg.get("d", "x"), None)
        self.assertEqual(cfg.get("d", "y"), "2")
        self.assertEqual(cfg.get("e", "x"), None)
        self.assertEqual(
            normalizeSources(cfg, "a", "x"), [("1", ("a.rc", 6, 7, 2), "readpath")]
        )
        self.assertEqual(
            normalizeSources(cfg, "a", "y"),
            [
                ("2", ("a.rc", 10, 11, 3), "readpath"),
                (None, ("b.rc", 35, 36, 5), "readpath"),
            ],
        )
        self.assertEqual(
            cfg.sources("c", "x"), [("1", ("<builtin>", 6, 7, 2), "parse")]
        )

    def testSectionIncludelist(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", ["a"], None, None)
        self.assertEqual(cfg.sections(), ["a"])

    def testSectionRemap(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, [("a", "x")], None)
        self.assertEqual(cfg.sections(), ["x", "b"])

    def testIncludelist(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, None, [("a", "y")])
        self.assertEqual(cfg.get("a", "x"), "1")
        self.assertEqual(cfg.get("a", "y"), None)

    def testClone(self):
        cfg1 = createConfig()
        cfg1.set("a", "x", "1", "set1")
        cfg2 = cfg1.clone()
        cfg2.set("b", "y", "2", "set2")
        self.assertEqual(cfg1.sections(), ["a"])
        self.assertEqual(cfg2.sections(), ["a", "b"])


if __name__ == "__main__":
    silenttestrunner.main(__name__)
