# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import os

def localpath(p):
    return p.lstrip('/')

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
        r[int(changelist)] = flag.encode('ascii')
    return r

def lastcl(node):
    if node:
        assert node.extra().get('p4changelist')
        return int(node.extra()['p4changelist']) + 1
    return None
