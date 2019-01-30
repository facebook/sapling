# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

from edenscm.mercurial import extensions


lfsext = None
try:
    lfsext = extensions.find("lfs")
except KeyError:
    pass


def getlfsinfo(hgfilelog, node):
    if lfsext is None:
        return False, None
    islfs = lfsext.wrapper._islfs(hgfilelog, node)
    oid = None
    if islfs:
        lfspointer = lfsext.pointer.deserialize(hgfilelog.revision(node, raw=True))
        oid = lfspointer.oid()
    return islfs, oid
