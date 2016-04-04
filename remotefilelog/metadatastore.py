import os
import basestore, ioutil
from mercurial import util
from mercurial.node import hex

class remotefilelogmetadatastore(basestore.basestore):
    def getparents(self, name, node):
        """Returns the immediate parents of the node."""
        pass

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode),
           ...
        }
        """
        pass

    def getlinknode(self, name, node):
        pass

    def add(self, name, node, parents, linknode):
        raise Exception("cannot add metadata only to remotefilelog "
                        "metadatastore")
