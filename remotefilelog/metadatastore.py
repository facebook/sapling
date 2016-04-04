import os
import basestore, shallowutil
from mercurial import util
from mercurial.node import hex

class unionmetadatastore(object):
    def __init__(self, local, shared):
        self._local = local
        self._shared = shared

    def getparents(self, name, node):
        """Returns the immediate parents of the node."""
        ancestors = self.getancestors(name, node)
        return ancestors[node][:2]

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode),
           ...
        }
        """
        try:
            return self._shared.getancestors(name, node)
        except KeyError:
            pass

        try:
            return self._local.getancestors(name, node)
        except KeyError:
            pass

        self._shared.triggerfetches([(name, node)])
        try:
            return self._shared.getancestors(name, node)
        except KeyError:
            pass

        raise error.LookupError(node, name, _('no valid file history'))

    def getlinknode(self, name, node):
        ancestors = self.getancestors(name, node)
        return ancestors[node][3]

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")

    def contains(self, keys):
        missing = self._local.contains(keys)
        if missing:
            missing = self._shared.contains(missing)
        return missing

    def addfetcher(self, fetchfunc):
        self._shared.addfetcher(fetchfunc)

    def triggerfetches(self, keys):
        self._shared.triggerfetches(keys)

class remotefilelogmetadatastore(basestore.basestore):
    def getparents(self, name, node):
        """Returns the immediate parents of the node."""
        ancestors = self.getancestors(name, node)
        return ancestors[node][:2]

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode),
           ...
        }
        """
        data = self._getdata(name, node)
        ancestors = shallowutil.ancestormap(data)
        return ancestors

    def getlinknode(self, name, node):
        ancestors = self.getancestors(name, node)
        return ancestors[node][3]

    def add(self, name, node, parents, linknode):
        raise Exception("cannot add metadata only to remotefilelog "
                        "metadatastore")
