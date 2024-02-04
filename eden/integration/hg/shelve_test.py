#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


log = logging.getLogger("eden.test.shelve")


@hg_test
# pyre-ignore[13]: T62487924
class ShelveTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("foo", "A")
        repo.commit("Initial commit.")

    def test_shelve_unshelve(self) -> None:
        self.assert_status_empty()

        self.write_file("foo", "B")
        self.assert_status({"foo": "M"})

        self.hg("shelve")
        self.assert_status_empty()

        self.write_file("foo", "C")
        self.assert_status({"foo": "M"})

        self.hg("unshelve", "--tool", ":other")
        self.assert_status({"foo": "M"})
        self.assertEqual("B", self.read_file("foo"))
