import os, shutil
import basestore, ioutil
from mercurial import util
from mercurial.node import hex

class unioncontentstore(object):
    def __init__(self, local, shared):
        self._local = local
        self._shared = shared

    def get(self, name, node):
        try:
            return self._shared.get(name, node)
        except KeyError:
            pass

        try:
            return self._local.get(name, node)
        except KeyError:
            pass

        self._shared.triggerfetches([(name, node)])
        try:
            return self._shared.get(name, node)
        except KeyError:
            pass

        raise error.LookupError(id, self.filename, _('no node'))

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")

    def contains(self, keys):
        missing = self._local.contains(keys)
        if missing:
            missing = self._shared.contains(missing)
        return missing

    def addremotefilelog(self, name, node, data):
        self._local.addremotefilelog(name, node, data)

    def addfetcher(self, fetchfunc):
        self._shared.addfetcher(fetchfunc)

    def triggerfetches(self, keys):
        self._shared.triggerfetches(keys)

class remotefilelogcontentstore(basestore.basestore):
    def get(self, name, node):
        data = self._getdata(name, node)

        index, size = ioutil.parsesize(data)
        content = data[(index + 1):(index + 1 + size)]

        ancestormap = ioutil.ancestormap(data)
        p1, p2, linknode, copyfrom = ancestormap[node]
        copyrev = None
        if copyfrom:
            copyrev = hex(p1)

        revision = ioutil.createrevlogtext(content, copyfrom, copyrev)
        return revision

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")
