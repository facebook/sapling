#!/usr/bin/env python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

import bindings
import silenttestrunner
from sapling.node import bin


class ManifestInsertBeforeRemoveTest(unittest.TestCase):
    def setup(self):
        manifest = bindings.manifest.testtreemanifest()
        manifest["dir/name"] = translatenode("1")
        manifest["dir/unchanged"] = translatenode("2")
        manifest.finalize()
        return manifest

    def testInsertDirectoryInPlaceOfFile(self):
        manifest = self.setup()
        manifest["dir/name/file"] = translatenode("3")
        del manifest["dir/name"]
        manifest.finalize()

    def testInsertFileInPlaceOfDirector(self):
        manifest = self.setup()
        manifest["dir"] = translatenode("3")
        del manifest["dir/name"]
        del manifest["dir/unchanged"]
        manifest.finalize()

    def testFinalizeWithoutDeleting(self):
        manifest = self.setup()
        manifest["dir/name/file"] = translatenode("3")
        try:
            manifest.finalize()
            raise RuntimeError(
                "manifest.finalize is expected to throw when there are "
                "lingering paths should have been removed (dir/name)"
            )
        except RuntimeError:
            pass  # expected to raise


def translatenode(value):
    value = value.rjust(40, "0")
    return bin(value)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
