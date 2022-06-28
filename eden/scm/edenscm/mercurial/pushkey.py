# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pushkey.py - dispatching for pushing and pulling keys
#
# Copyright 2010 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import bookmarks, phases, util
from .pycompat import decodeutf8, encodeutf8


def _nslist(repo):
    n = {}
    for k in _namespaces:
        n[k] = ""
    return n


_namespaces = {
    "namespaces": (lambda *x: False, _nslist),
    "bookmarks": (bookmarks.pushbookmark, bookmarks.listbookmarks),
    "phases": (phases.pushphase, phases.listphases),
}


def register(namespace, pushkey, listkeys):
    _namespaces[namespace] = (pushkey, listkeys)


def _get(namespace):
    return _namespaces.get(namespace, (lambda *x: False, lambda *x: {}))


def push(repo, namespace, key, old, new):
    """should succeed iff value was old"""
    pk = _get(namespace)[0]
    return pk(repo, key, old, new)


def list(repo, namespace):
    """return a dict"""
    lk = _get(namespace)[1]
    return lk(repo)


def encodekeys(keys):
    """encode the content of a pushkey namespace for exchange over the wire"""
    return b"\n".join([b"%s\t%s" % (encodeutf8(k), encodeutf8(v)) for k, v in keys])


def decodekeys(data):
    """decode the content of a pushkey namespace from exchange over the wire"""
    # Note that the order is required in some cases. E.g. pullbackup needs to
    # retrieve commits in the same order of creation to mantain the order of
    # revision codes. See T24417531
    result = util.sortdict()
    for l in data.splitlines():
        k, v = l.split(b"\t")
        result[decodeutf8(k)] = decodeutf8(v)
    return result
