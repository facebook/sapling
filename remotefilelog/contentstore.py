import os, shutil
import basestore, shallowutil
from mercurial import util
from mercurial.node import hex

class unioncontentstore(object):
    def __init__(self, local, shared, remote):
        self._local = local
        self._shared = shared
        self._remote = remote

    def get(self, name, node):
        try:
            return self._shared.get(name, node)
        except KeyError:
            pass

        try:
            return self._local.get(name, node)
        except KeyError:
            pass

        try:
            return self._remote.get(name, node)
        except KeyError:
            pass

        raise error.LookupError(id, self.filename, _('no node'))

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")

    def getmissing(self, keys):
        missing = self._local.getmissing(keys)
        if missing:
            missing = self._shared.getmissing(missing)
        return missing

    def addremotefilelognode(self, name, node, data):
        self._local.addremotefilelognode(name, node, data)

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

    def contains(self, keys):
        raise NotImplemented()
