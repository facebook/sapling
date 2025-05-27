# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from . import shallowutil


class unionmetadatastore:
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
        raise RuntimeError("cannot add content only to remotefilelog contentstore")

    def getmissing(self, keys):
        missing = keys
        for store in self.stores:
            if missing:
                missing = store.getmissing(missing)
        return missing

    def markforrefresh(self):
        for store in self.stores:
            if hasattr(store, "markforrefresh"):
                store.markforrefresh()

    def addstore(self, store):
        self.stores.append(store)

    def removestore(self, store):
        self.stores.remove(store)

    def flush(self):
        for store in self.stores:
            if hasattr(store, "flush"):
                store.flush()
