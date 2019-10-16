# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import util
from edenscm.mercurial.node import hex, nullid

from . import shallowutil


class unionmetadatastore(object):
    def __init__(self, *args, **kwargs):
        self.stores = list(args)

        # If allowincomplete==True then the union store can return partial
        # ancestor lists, otherwise it will throw a KeyError if a full
        # history can't be found.
        self.allowincomplete = kwargs.get("allowincomplete", False)

    def getancestors(self, name, node, known=None):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode, copyfrom),
           ...
        }
        """
        if known is None:
            known = set()
        if node in known:
            return []

        ancestors = {}

        def traverse(curname, curnode):
            # TODO: this algorithm has the potential to traverse parts of
            # history twice. Ex: with A->B->C->F and A->B->D->F, both D and C
            # may be queued as missing, then B and A are traversed for both.
            queue = [(curname, curnode)]
            missing = []
            seen = set()
            while queue:
                name, node = queue.pop()
                if (name, node) in seen:
                    continue
                seen.add((name, node))
                value = ancestors.get(node)
                if not value:
                    missing.append((name, node))
                    continue
                p1, p2, linknode, copyfrom = value
                if p1 != nullid and p1 not in known:
                    queue.append((copyfrom or name, p1))
                if p2 != nullid and p2 not in known:
                    queue.append((name, p2))
            return missing

        missing = [(name, node)]
        while missing:
            curname, curnode = missing.pop()
            try:
                ancestors.update(
                    self._getpartialancestors(curname, curnode, known=known)
                )
                newmissing = traverse(curname, curnode)
                missing.extend(newmissing)
            except KeyError:
                # If we allow incomplete histories, don't throw.
                if not self.allowincomplete:
                    raise
                # If the requested name+node doesn't exist, always throw.
                if (curname, curnode) == (name, node):
                    raise

        # TODO: ancestors should probably be (name, node) -> (value)
        return ancestors

    def _getpartialancestors(self, name, node, known=None):
        for store in self.stores:
            try:
                return store.getancestors(name, node, known=known)
            except KeyError:
                pass

        raise shallowutil.MissingNodesError([(name, node)])

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

    def getancestors(self, name, node, known=None):
        self._prefetch(name, node)
        return self._shared.getancestors(name, node, known=known)

    def getnodeinfo(self, name, node):
        self._prefetch(name, node)
        return self._shared.getnodeinfo(name, node)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a remote store")

    def getmissing(self, keys):
        return keys
