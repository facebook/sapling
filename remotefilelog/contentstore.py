import basestore, shallowutil
from mercurial import mdiff, util
from mercurial.node import hex, nullid

class ChainIndicies(object):
    """A static class for easy reference to the delta chain indicies.
    """
    # The filename of this revision delta
    NAME = 0
    # The mercurial file node for this revision delta
    NODE = 1
    # The filename of the delta base's revision. This is useful when delta
    # between different files (like in the case of a move or copy, we can delta
    # against the original file content).
    BASENAME = 2
    # The mercurial file node for the delta base revision. This is the nullid if
    # this delta is a full text.
    BASENODE = 3
    # The actual delta or full text data.
    DATA = 4

class unioncontentstore(object):
    def __init__(self, *args, **kwargs):
        self.stores = args
        self.writestore = kwargs.get('writestore')

        # If allowincomplete==True then the union store can return partial
        # delta chains, otherwise it will throw a KeyError if a full
        # deltachain can't be found.
        self.allowincomplete = kwargs.get('allowincomplete', False)

    def get(self, name, node):
        """Fetches the full text revision contents of the given name+node pair.
        If the full text doesn't exist, throws a KeyError.

        Under the hood, this uses getdeltachain() across all the stores to build
        up a full chain to produce the full text.
        """
        chain = self.getdeltachain(name, node)

        if chain[-1][ChainIndicies.BASENODE] != nullid:
            # If we didn't receive a full chain, throw
            raise KeyError((name, hex(node)))

        # The last entry in the chain is a full text, so we start our delta
        # applies with that.
        fulltext = chain.pop()[ChainIndicies.DATA]

        text = fulltext
        while chain:
            delta = chain.pop()[ChainIndicies.DATA]
            text = mdiff.patches(text, [delta])

        return text

    def getdeltachain(self, name, node):
        """Returns the deltachain for the given name/node pair.

        Returns an ordered list of:

          [(name, node, deltabasename, deltabasenode, deltacontent),...]

        where the chain is terminated by a full text entry with a nullid
        deltabasenode.
        """
        chain = self._getpartialchain(name, node)
        while chain[-1][ChainIndicies.BASENODE] != nullid:
            x, x, deltabasename, deltabasenode, x = chain[-1]
            try:
                morechain = self._getpartialchain(deltabasename, deltabasenode)
                chain.extend(morechain)
            except KeyError:
                # If we allow incomplete chains, don't throw.
                if not self.allowincomplete:
                    raise
                break

        return chain

    def _getpartialchain(self, name, node):
        """Returns a partial delta chain for the given name/node pair.

        A partial chain is a chain that may not be terminated in a full-text.
        """
        for store in self.stores:
            try:
                return store.getdeltachain(name, node)
            except KeyError:
                pass

        raise KeyError((name, hex(node)))

    def add(self, name, node, data):
        raise RuntimeError("cannot add content only to remotefilelog "
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
            raise RuntimeError("no writable store configured")

    def markledger(self, ledger):
        for store in self.stores:
            store.markledger(ledger)

    def markforrefresh(self):
        for store in self.stores:
            if util.safehasattr(store, 'markforrefresh'):
                store.markforrefresh()

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

    def getdeltachain(self, name, node):
        # Since remotefilelog content stores just contain full texts, we return
        # a fake delta chain that just consists of a single full text revision.
        # The nullid in the deltabasenode slot indicates that the revision is a
        # fulltext.
        revision = self.get(name, node)
        return [(name, node, None, nullid, revision)]

    def add(self, name, node, data):
        raise RuntimeError("cannot add content only to remotefilelog "
                           "contentstore")

class remotecontentstore(object):
    def __init__(self, ui, fileservice, shared):
        self._fileservice = fileservice
        self._shared = shared

    def get(self, name, node):
        self._fileservice.prefetch([(name, hex(node))], force=True,
                                   fetchdata=True)
        return self._shared.get(name, node)

    def getdeltachain(self, name, node):
        # Since our remote content stores just contain full texts, we return a
        # fake delta chain that just consists of a single full text revision.
        # The nullid in the deltabasenode slot indicates that the revision is a
        # fulltext.
        revision = self.get(name, node)
        return [(name, node, None, nullid, revision)]

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a remote store")

    def getmissing(self, keys):
        return keys

    def markledger(self, ledger):
        pass
