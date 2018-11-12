from mercurial.node import hex


# Offsets in the mutable pack tuple
DATA = 0
HISTORY = 1


class mutabledatahistorystore(object):
    """A proxy class that gets added to the union store and knows how to answer
    requests by inspecting the current mutable data and history packs. We can't
    insert the mutable packs themselves into the union store because they can be
    created and destroyed over time."""

    def __init__(self, log, shared=False):
        self.log = log
        self.shared = shared

    def _packs(self):
        if self.shared:
            return self.log._mutablesharedpacks
        else:
            return self.log._mutablelocalpacks

    def getmissing(self, keys):
        packs = self._packs()
        if packs is None:
            return keys

        return packs[DATA].getmissing(keys)

    def get(self, name, node):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[DATA].get(name, node)

    def getdelta(self, name, node):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[DATA].getdelta(name, node)

    def getdeltachain(self, name, node):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[DATA].getdeltachain(name, node)

    def getmeta(self, name, node):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[DATA].getmeta(name, node)

    def getnodeinfo(self, name, node):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[HISTORY].getnodeinfo(name, node)

    def getancestors(self, name, node, known=None):
        packs = self._packs()
        if packs is None:
            raise KeyError(name, hex(node))

        return packs[HISTORY].getancestors(name, node, known=known)

    def getmetrics(self):
        return {}
