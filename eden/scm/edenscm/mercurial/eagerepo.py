# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import bindings

from . import error, filelog, revlog
from .i18n import _
from .node import bin, nullid

EAGEREPO_REQUIREMENT = "eagerepo"


def iseagerepo(repo):
    return EAGEREPO_REQUIREMENT in repo.storerequirements


def openstore(repo):
    from .changelog2 import HGCOMMITS_DIR

    path = repo.svfs.join(HGCOMMITS_DIR)
    return bindings.eagerepo.EagerRepoStore.open(path)


class eagerfilelog(object):
    """filelog-like interface for EagerRepoStore"""

    def __init__(self, repo, name):
        self.store = repo.fileslog.contentstore
        self.name = name

    def lookup(self, node):
        assert len(node) == 20
        return node

    def read(self, node):
        t = self._get_content(node)
        # see filelog.read - strip filelog metadata
        if not t.startswith(b"\1\n"):
            return t
        else:
            s = t.index(b"\1\n", 2)
            return t[s + 2 :]

    def size(self, node):
        return len(self.read(node))

    def rev(self, node):
        # same trick as remotefilelog
        return node

    def cmp(self, node, text):
        """returns True if blob hash is different from text"""
        # PERF: This does use a fast path avoid read() - a fast path requires
        # fast path reading p1, p2, which does not exist.
        return self.read(node) != text

    def renamed(self, node):
        t = self._get_content(node)
        if not t.startswith(b"\1\n"):
            return False
        m = filelog.parsemeta(t)[0]
        if m and "copy" in m:
            return (m["copy"], bin(m["copyrev"]))
        return False

    def add(self, text, meta, _tr, _linkrev, fparent1, fparent2):
        # see filelog.add and revlog.addrevision
        if meta or text.startswith(b"\1\n"):
            text = filelog.packmeta(meta, text)
        rawtext = revlog.textwithheader(text, fparent1, fparent2)
        # SPACE: didn't set the "bases" for candidate delta bases.
        node = self.store.add_sha1_blob(rawtext)
        return node

    def _get_content(self, node):
        if node == nullid:
            return b""
        t = self.store.get_sha1_blob(node)
        if t is None:
            raise error.LookupError(node, self.name, _("no node"))
        return t
