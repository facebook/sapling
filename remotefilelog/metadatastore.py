import os
import basestore, shallowutil
from mercurial import util
from mercurial.node import hex

class unionmetadatastore(object):
    def __init__(self, local, shared, remote):
        self._local = local
        self._shared = shared
        self._remote = remote

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

        try:
            return self._remote.getancestors(name, node)
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

class remotemetadatastore(object):
    def __init__(self, ui, fileservice, shared):
        self._fileservice = fileservice
        self._shared = shared

    def getancestors(self, name, node):
        self._fileservice.prefetch([(name, hex(node))])
        return self._shared.getancestors(name, node)

    def add(self, name, node, data):
        raise Exception("cannot add to a remote store")

    def contains(self, keys):
        raise NotImplemented()

    def getparents(self, name, node):
        raise NotImplemented()

    def getlinknode(self, name, node):
        raise NotImplemented()
