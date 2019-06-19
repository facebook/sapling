#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import configparser
import unittest

from .. import configinterpolator


class InterpolatorTest(unittest.TestCase):
    def test_basic_subs(self):
        defaults = {"USER": "wez", "RECURSIVE": "a${RECURSIVE}b"}
        parser = configparser.ConfigParser(
            interpolation=configinterpolator.EdenConfigInterpolator(defaults)
        )
        parser.add_section("section")
        parser.set("section", "user", "${USER}")
        parser.set("section", "rec", "${RECURSIVE}")
        parser.set("section", "simple", "value")

        self.assertEqual("wez", parser.get("section", "user"))
        self.assertEqual("value", parser.get("section", "simple"))
        self.assertEqual("a${RECURSIVE}b", parser.get("section", "rec"))

        actual = {}
        for section in parser.sections():
            actual[section] = dict(parser.items(section))

        expect = {
            "section": {"user": "wez", "simple": "value", "rec": "a${RECURSIVE}b"}
        }
        self.assertEqual(expect, actual)
