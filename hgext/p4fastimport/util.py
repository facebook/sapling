# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import os

from mercurial import util, worker


def localpath(p):
    return p.lstrip("/")


def storepath(b, p, ci=False):
    p = os.path.join(b, p)
    if ci:
        p = p.lower()
    return p


def caseconflict(filelist):
    temp = {}
    conflicts = []
    for this in filelist:
        if this.lower() in temp:
            other = temp[this.lower()]
            if this != other:
                conflicts.append(sorted([this, other]))
        temp[this.lower()] = this
    return sorted(conflicts)


def decodefileflags(json):
    r = collections.defaultdict(dict)
    for changelist, flag in json.items():
        r[int(changelist)] = flag.encode("ascii")
    return r


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


def runworker(ui, fn, wargs, items):
    # 0.4 is the cost per argument. So if we have at least 100 files
    # on a 4 core machine than our linear cost outweights the
    # drawback of spwaning. We are overwritign this if we force a
    # worker to run with a ridiculous high number.
    weight = 0.0  # disable worker
    useworker = ui.config("p4fastimport", "useworker")
    if useworker == "force":
        weight = 100000.0  # force worker
    elif util.parsebool(useworker or ""):
        weight = 0.04  # normal weight

    # Fix duplicated messages before
    # https://www.mercurial-scm.org/repo/hg-committed/rev/9d3d56aa1a9f
    ui.flush()
    return worker.worker(ui, weight, fn, wargs, items)
