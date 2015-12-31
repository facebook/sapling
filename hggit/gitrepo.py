from util import isgitsshuri
from mercurial import util
from mercurial.error import RepoError
from mercurial.peer import peerrepository

class gitrepo(peerrepository):
    capabilities = ['lookup']

    def _capabilities(self):
        return self.capabilities

    def __init__(self, ui, path, create):
        if create:  # pragma: no cover
            raise util.Abort('Cannot create a git repository.')
        self.ui = ui
        self.path = path
        self.localrepo = None

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
            return {'bookmarks': ''}
        elif namespace == 'bookmarks':
            if self.localrepo is not None:
                handler = self.localrepo.githandler
                refs = handler.fetch_pack(self.path, heads=[])
                # map any git shas that exist in hg to hg shas
                stripped_refs = dict([
                    (ref[11:], handler.map_hg_get(refs[ref]) or refs[ref])
                    for ref in refs.keys() if ref.startswith('refs/heads/')
                ])
                return stripped_refs
        return {}

    def pushkey(self, namespace, key, old, new):
        return False

instance = gitrepo

def islocal(path):
    if isgitsshuri(path):
        return True

    u = util.url(path)
    return not u.scheme or u.scheme == 'file'
