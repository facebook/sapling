from edenscm.mercurial import error, util
from edenscm.mercurial.error import RepoError
from util import isgitsshuri


peerapi = False
try:
    from edenscm.mercurial.repository import peer as peerrepository

    peerapi = True
except ImportError:
    from edenscm.mercurial.peer import peerrepository


class gitrepo(peerrepository):
    def __init__(self, ui, path, create):
        if create:  # pragma: no cover
            raise error.Abort("Cannot create a git repository.")
        self._ui = ui
        self.path = path
        self.localrepo = None

    _peercapabilities = ["lookup"]

    def _capabilities(self):
        return self._peercapabilities

    def capabilities(self):
        return self._peercapabilities

    @property
    def ui(self):
        return self._ui

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
        if namespace == "namespaces":
            return {"bookmarks": ""}
        elif namespace == "bookmarks":
            if self.localrepo is not None:
                handler = self.localrepo.githandler
                refs = handler.fetch_pack(self.path, heads=[])
                # map any git shas that exist in hg to hg shas
                stripped_refs = dict(
                    [
                        (ref[11:], handler.map_hg_get(refs[ref]) or refs[ref])
                        for ref in refs.keys()
                        if ref.startswith("refs/heads/")
                    ]
                )
                return stripped_refs
        return {}

    def pushkey(self, namespace, key, old, new):
        return False

    if peerapi:

        def branchmap(self):
            raise NotImplementedError

        def canpush(self):
            return True

        def close(self):
            pass

        def debugwireargs(self):
            raise NotImplementedError

        def getbundle(self):
            raise NotImplementedError

        def iterbatch(self):
            raise NotImplementedError

        def known(self):
            raise NotImplementedError

        def peer(self):
            return self

        def stream_out(self):
            raise NotImplementedError

        def unbundle(self):
            raise NotImplementedError


instance = gitrepo


def islocal(path):
    if isgitsshuri(path):
        return True

    u = util.url(path)
    return not u.scheme or u.scheme == "file"
