import os
from mercurial import util
try:
    from mercurial.error import RepoError
except ImportError:
    from mercurial.repo import RepoError

try:
    from mercurial.peer import peerrepository
except ImportError:
    from mercurial.repo import repository as peerrepository

from git_handler import GitHandler

from overlay import overlayrepo

from mercurial.node import bin

class gitrepo(peerrepository):
    capabilities = ['lookup']

    def _capabilities(self):
        return self.capabilities

    def __init__(self, ui, path, create):
        if create: # pragma: no cover
            raise util.Abort('Cannot create a git repository.')
        self.ui = ui
        self.path = path
        self.localrepo = None
        self.handler = None

    def _initializehandler(self):
        if self.handler is None and self.localrepo is not None:
            self.handler = GitHandler(self.localrepo, self.localrepo.ui)
        return self.handler

    def url(self):
        return self.path

    def lookup(self, key):
        if isinstance(key, str):
            return key

    def local(self):
        if not self.path:
            raise RepoError

    def heads(self):
        return []

    def listkeys(self, namespace):
        if namespace == 'namespaces':
            return {'bookmarks':''}
        elif namespace == 'bookmarks':
            handler = self._initializehandler()
            if handler:
                handler.export_commits()
                refs = handler.fetch_pack(self.path)
                reqrefs = refs
                convertlist, commits = handler.getnewgitcommits(reqrefs)
                newcommits = [bin(c) for c in commits]
                b = overlayrepo(handler, newcommits, refs)
                stripped_refs = dict([
                    (ref[ref.find('refs/heads/')+11:], b.node(refs[ref]))
                        for ref in refs.keys()
                            if ref.startswith('refs/heads/')])
                return stripped_refs
        return {}

    def pushkey(self, namespace, key, old, new):
        return False

    # used by incoming in hg <= 1.6
    def branches(self, nodes):
        return []

instance = gitrepo

def islocal(path):
    u = util.url(path)
    return not u.scheme or u.scheme == 'file'
