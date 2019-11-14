# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import util
from edenscm.mercurial.node import hex, nullid

from . import shallowutil


class unionmetadatastore(object):
    def __init__(self, *args):
        self.stores = list(args)

    def getnodeinfo(self, name, node):
        for store in self.stores:
            try:
                return store.getnodeinfo(name, node)
            except KeyError:
                pass

        raise shallowutil.MissingNodesError([(name, node)])

    def add(self, name, node, data):
        raise RuntimeError("cannot add content only to remotefilelog " "contentstore")

    def getmissing(self, keys):
        missing = keys
        for store in self.stores:
            if missing:
                missing = store.getmissing(missing)
        return missing

    def getmetrics(self):
        metrics = [s.getmetrics() for s in self.stores]
        return shallowutil.sumdicts(*metrics)

    def markforrefresh(self):
        for store in self.stores:
            if util.safehasattr(store, "markforrefresh"):
                store.markforrefresh()

    def addstore(self, store):
        self.stores.append(store)

    def removestore(self, store):
        self.stores.remove(store)


class remotemetadatastore(object):
    def __init__(self, ui, fileservice, shared):
        self._fileservice = fileservice
        self._shared = shared

    def _prefetch(self, name, node):
        self._fileservice.prefetch(
            [(name, hex(node))], force=True, fetchdata=False, fetchhistory=True
        )

    def getnodeinfo(self, name, node):
        self._prefetch(name, node)
        return self._shared.getnodeinfo(name, node)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a remote store")

    def getmissing(self, keys):
        return keys
