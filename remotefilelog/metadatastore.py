import os
import basestore, shallowutil
from mercurial import error, util
from mercurial.i18n import _
from mercurial.node import hex, nullid

class unionmetadatastore(object):
    def __init__(self, *args, **kwargs):
        self.stores = args
        self.writestore = kwargs.get('writestore')

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode, copyfrom),
           ...
        }
        """
        ancestors = {}
        def traverse(curname, curnode):
            # TODO: this algorithm has the potential to traverse parts of
            # history twice. Ex: with A->B->C->F and A->B->D->F, both D and C
            # may be queued as missing, then B and A are traversed for both.
            queue = [(curname, curnode)]
            missing = []
            while queue:
                name, node = queue.pop()
                value = ancestors.get(node)
                if not value:
                    missing.append((name, node))
                    continue
                p1, p2, linknode, copyfrom = value
                if p1 != nullid:
                    queue.append((copyfrom or curname, p1))
                if p2 != nullid:
                    queue.append((curname, p2))
            return missing

        missing = [(name, node)]
        while missing:
            curname, curnode = missing.pop()
            ancestors.update(self._getpartialancestors(curname, curnode))
            newmissing = traverse(curname, curnode)
            missing.extend(newmissing)

        # TODO: ancestors should probably be (name, node) -> (value)
        return ancestors

    def _getpartialancestors(self, name, node):
        for store in self.stores:
            try:
                return store.getancestors(name, node)
            except KeyError:
                pass

        raise error.LookupError(node, name, _('no valid file history'))

    def add(self, name, node, data):
        raise RuntimeError("cannot add content only to remotefilelog "
                           "contentstore")

    def getmissing(self, keys):
        missing = keys
        for store in self.stores:
            if missing:
                missing = store.getmissing(missing)
        return missing

    def markledger(self, ledger):
        for store in self.stores:
            store.markledger(ledger)

class remotefilelogmetadatastore(basestore.basestore):
    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode, copyfrom),
           ...
        }
        """
        data = self._getdata(name, node)
        ancestors = shallowutil.ancestormap(data)
        return ancestors

    def add(self, name, node, parents, linknode):
        raise RuntimeError("cannot add metadata only to remotefilelog "
                           "metadatastore")

class remotemetadatastore(object):
    def __init__(self, ui, fileservice, shared):
        self._fileservice = fileservice
        self._shared = shared

    def getancestors(self, name, node):
        self._fileservice.prefetch([(name, hex(node))], force=True,
                                   fetchdata=False, fetchhistory=True)
        return self._shared.getancestors(name, node)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a remote store")

    def getmissing(self, keys):
        return keys

    def markledger(self, ledger):
        pass
