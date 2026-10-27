# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core.sites import Site


class SiteTest(unittest.TestCase):
    def test_site_requires_daemon_and_behaviors(self) -> None:
        def validate(*_args: object) -> None:
            pass

        with self.assertRaises(ValueError):
            Site(
                key_class="key",
                target_kinds=frozenset(),
                behaviors=frozenset(),
                daemon="public-master",
                validate=validate,
                effect="pre",
            )
        with self.assertRaises(ValueError):
            Site(
                key_class="key",
                target_kinds=frozenset(),
                behaviors=frozenset({"block"}),
                daemon="unknown",
                validate=validate,
                effect="pre",
            )
        with self.assertRaises(ValueError):
            Site(
                key_class="key",
                target_kinds=frozenset(),
                behaviors=frozenset({"block"}),
                daemon="public-master",
                validate=validate,
                effect="during",
            )
