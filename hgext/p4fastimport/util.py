# (c) 2017-present Facebook Inc.
from __future__ import absolute_import


def localpath(p):
    return p.lstrip("/")


def getcl(node):
    if node:
        assert node.extra().get("p4changelist") or node.extra().get(
            "p4fullimportbasechangelist"
        )
        if node.extra().get("p4changelist"):
            return int(node.extra()["p4changelist"])
        else:
            return int(node.extra()["p4fullimportbasechangelist"])
    return None


def lastcl(node):
    clnum = getcl(node)
    if clnum:
        return clnum + 1
    return None
