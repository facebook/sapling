#!/usr/bin/env python
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

import bindings
import silenttestrunner
from edenscm.mercurial.node import bin


class ManifestInsertBeforeRemoveTest(unittest.TestCase):
    def setup(self):
        store = TestStore()
        manifest = bindings.manifest.treemanifest(store)
        manifest["dir/name"] = translatenode("1")
        manifest["dir/unchanged"] = translatenode("2")
        for (path, node, raw, _, _, _) in manifest.finalize():
            store.insert(path, node, raw)
        return (store, manifest)

    def testInsertDirectoryInPlaceOfFile(self):
        (store, manifest) = self.setup()
        manifest["dir/name/file"] = translatenode("3")
        del manifest["dir/name"]
        manifest.finalize()

    def testInsertFileInPlaceOfDirector(self):
        (store, manifest) = self.setup()
        manifest["dir"] = translatenode("3")
        del manifest["dir/name"]
        del manifest["dir/unchanged"]
        manifest.finalize()

    def testFinalizeWithoutDeleting(self):
        (store, manifest) = self.setup()
        manifest["dir/name/file"] = translatenode("3")
        try:
            manifest.finalize()
            raise RuntimeError(
                "manifest.finalize is expected to throw when there are "
                "lingering paths should have been removed (dir/name)"
            )
        except RuntimeError:
            pass  # expected to raise


class TestStore(object):
    def __init__(self):
        self.underlying = {}

    def get(self, key):
        try:
            return self.underlying[key.path][key.node]
        except KeyError:
            return None

    def insert(self, path, node, value):
        # it's funny that the apis are asymetrical
        if not path in self.underlying:
            self.underlying[path] = {}
        self.underlying[path][node] = value

    def prefetch(self, keys):
        pass


def translatenode(value):
    value = value.rjust(40, "0")
    return bin(value)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
