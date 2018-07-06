# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

from mercurial import extensions


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
