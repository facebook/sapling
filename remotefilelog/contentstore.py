import os, shutil
import basestore, shallowutil
from mercurial import util
from mercurial.node import hex

class unioncontentstore(object):
    def __init__(self, *args, **kwargs):
        self.stores = args
        self.writestore = kwargs.get('writestore')

    def get(self, name, node):
        for store in self.stores:
            try:
                return store.get(name, node)
            except KeyError:
                pass

        raise error.LookupError(id, self.filename, _('no node'))

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")

    def getmissing(self, keys):
        missing = keys
        for store in self.stores:
            if missing:
                missing = store.getmissing(missing)
        return missing

    def addremotefilelognode(self, name, node, data):
        if self.writestore:
            self.writestore.addremotefilelognode(name, node, data)
        else:
            raise Exception("no writable store configured")

class remotefilelogcontentstore(basestore.basestore):
    def get(self, name, node):
        data = self._getdata(name, node)

        index, size = shallowutil.parsesize(data)
        content = data[(index + 1):(index + 1 + size)]

        ancestormap = shallowutil.ancestormap(data)
        p1, p2, linknode, copyfrom = ancestormap[node]
        copyrev = None
        if copyfrom:
            copyrev = hex(p1)

        revision = shallowutil.createrevlogtext(content, copyfrom, copyrev)
        return revision

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")

class remotecontentstore(object):
    def __init__(self, ui, fileservice, shared):
        self._fileservice = fileservice
        self._shared = shared

    def get(self, name, node):
        self._fileservice.prefetch([(name, hex(node))])
        return self._shared.get(name, node)

    def add(self, name, node, data):
        raise Exception("cannot add to a remote store")

    def getmissing(self, keys):
        return keys
