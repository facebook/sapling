from __future__ import absolute_import

import os
import random
import shutil
import tempfile
import unittest

import silenttestrunner
from edenscm.mercurial import node
from edenscmnative.bindings import bookmarkstore


class bookmarkstoretests(unittest.TestCase):
    def setUp(self):
        random.seed(0)
        self._tempdirs = []

    def tearDown(self):
        for d in self._tempdirs:
            shutil.rmtree(d)

    def makeTempDir(self):
        tempdir = tempfile.mkdtemp()
        self._tempdirs.append(tempdir)
        return tempdir

    def testAddingBookmarks(self):
        bmdir = self.makeTempDir()
        bmstore = bookmarkstore.bookmarkstore(bmdir)
        self.assertIsNone(bmstore.lookup_bookmark("not_real"))

        bmstore.update("test", node.nullid)
        self.assertEquals(
            "\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
            bmstore.lookup_bookmark("test"),
        )

        bmstore.update("test", node.bin("1" * 40))
        self.assertEquals(
            "\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11",
            bmstore.lookup_bookmark("test"),
        )

        bmstore.remove("test")

    def testAddingBookmarksToSameNode(self):
        bmdir = self.makeTempDir()
        bmstore = bookmarkstore.bookmarkstore(bmdir)

        testnode = node.bin("2" * 40)
        self.assertIsNone(bmstore.lookup_node(testnode))

        bmstore.update("test", testnode)
        bmstore.update("test2", testnode)

        self.assertEquals(["test2", "test"], bmstore.lookup_node(testnode))

        bmstore.remove("test2")
        self.assertEquals(["test"], bmstore.lookup_node(testnode))

    def testMalformedBookmarks(self):
        bmdir = self.makeTempDir()
        bmstore = bookmarkstore.bookmarkstore(bmdir)
        bmstore.update("test", node.bin("1" * 40))
        bmstore.flush()

        def truncateFilesInDir(d):
            for f in os.listdir(d):
                with open(os.path.join(d, f), "w"):
                    pass

        truncateFilesInDir(bmdir)
        self.assertRaises(IOError, bookmarkstore.bookmarkstore, bmdir)

    def testLoadingBookmarks(self):
        bmdir = self.makeTempDir()
        bmstore1 = bookmarkstore.bookmarkstore(bmdir)
        bmstore1.update("test", node.bin("1" * 40))
        bmstore1.flush()

        bmstore2 = bookmarkstore.bookmarkstore(bmdir)
        self.assertEquals(
            "\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11",
            bmstore2.lookup_bookmark("test"),
        )

    def testFlushingBookmarks(self):
        bmdir = self.makeTempDir()
        bmstore = bookmarkstore.bookmarkstore(bmdir)
        bmstore.update("test", node.bin("1" * 40))
        bmstore.flush()
        self.assertTrue(len(os.listdir(bmdir)) > 0)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
