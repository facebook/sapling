# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import unittest

import silenttestrunner
from bindings import configparser
from edenscm.mercurial.uiconfig import localrcfg
from edenscm.mercurial.util import writefile
from testutil.autofix import eq
from testutil.dott import testtmp  # noqa: F401


writefile(
    "a.rc",
    br"""[a]
x=1
y=2
%include b.rc
""",
)

writefile(
    "b.rc",
    br"""%include b.rc
[b]
z = 3
[a]
%unset y
%include broken.rc
""",
)

writefile("broken.rc", br"%not-implemented")


def createConfig():
    return localrcfg(configparser.config())


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


class ConfigParserTests(unittest.TestCase):
    def testReadConfig(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, None, None)
        cfg.parse("[c]\nx=1", "parse")
        cfg.set("d", "y", "2", "set1")
        cfg.set("d", "x", None, "set2")
        eq(cfg.sections(), ["a", "b", "c", "d"])
        eq(cfg.get("a", "x"), "1")
        eq(cfg.get("a", "y"), None)
        eq(cfg.get("b", "z"), "3")
        eq(cfg.get("c", "x"), "1")
        eq(cfg.get("d", "x"), None)
        eq(cfg.get("d", "y"), "2")
        eq(cfg.get("e", "x"), None)
        eq(normalizeSources(cfg, "a", "x"), [("1", ("a.rc", 6, 7, 2), "readpath")])
        eq(
            normalizeSources(cfg, "a", "y"),
            [
                ("2", ("a.rc", 10, 11, 3), "readpath"),
                (None, ("b.rc", 29, 36, 5), "readpath"),
            ],
        )
        eq(cfg.sources("c", "x"), [("1", ("<builtin>", 6, 7, 2), "parse")])

    def testSectionIncludelist(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", ["a"], None, None)
        eq(cfg.sections(), ["a"])

    def testSectionRemap(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, [("a", "x")], None)
        eq(cfg.sections(), ["x", "b"])

    def testIncludelist(self):
        cfg = createConfig()
        cfg.readpath("a.rc", "readpath", None, None, [("a", "y")])
        eq(cfg.get("a", "x"), "1")
        eq(cfg.get("a", "y"), None)

    def testClone(self):
        cfg1 = createConfig()
        cfg1.set("a", "x", "1", "set1")
        cfg2 = cfg1.clone()
        cfg2.set("b", "y", "2", "set2")
        eq(cfg1.sections(), ["a"])
        eq(cfg2.sections(), ["a", "b"])


if __name__ == "__main__":
    silenttestrunner.main(__name__)
