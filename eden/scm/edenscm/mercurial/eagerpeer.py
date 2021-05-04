# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import bindings
from edenscm import tracing

from . import repository, util, peer, error, bookmarks as bookmod
from .i18n import _
from .node import hex, bin

EagerRepo = bindings.eagerepo.EagerRepo


class eagerpeer(repository.peer):
    """peer backed by Rust EagerRepo (partially backed by EdenAPI)

    The EagerRepo is intended to be:
    - in Pure Rust
    - providing modern EdenAPI features
    - targeting modern client setups (ex. remotefilelog, treemanifest, segmented changelog)
    - avoiding tech-debts in bundle/wireproto if possible

    Eventually, this might evolve into an EdenAPI peer that works for
    anything implementing EdenApi in Rust, and *all* other kinds of
    remote peers can be deprecated. Currently, push related APIs are
    missing.
    """

    def __init__(self, ui, path, create=True):
        super(eagerpeer, self).__init__()

        url = util.url(path)
        assert url.scheme == "eager"

        self._path = url.path
        self._ui = ui
        self._reponame = "dummy"
        self._reload()

    def _reload(self):
        self._inner = EagerRepo.open(self._path)
        # Invalidate propertycache.
        for name in ("dag", "edenapi"):
            self.__dict__.pop(name, None)

    def _flush(self):
        self._inner.flush()
        tracing.debug("flushed")
        self._reload()

    # Modern interfaces backed by Rust

    @util.propertycache
    def dag(self):
        return self._inner.dag()

    @util.propertycache
    def edenapi(self):
        return self._inner.edenapiclient()

    # The Python "peer" interface.
    # Prefer using EdenAPI to implement them.

    @util.propertycache
    def ui(self):
        return self._ui

    def url(self):
        return "eager:%s" % self._path

    def local(self):
        return None

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        self._inner.flush()

    def branchmap(self):
        return {"default": self.heads()}

    def capabilities(self):
        return {"edenapi", "lookup", "pushkey", "known", "branchmap"}

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        return "%s %s %s %s %s" % (one, two, three, four, five)

    def getbundle(self, source, **kwargs):
        raise NotImplementedError()

    def heads(self):
        # Legacy API. Should not be used if selectivepull is on.
        heads = list(self.dag.heads(self.dag.all()))
        tracing.debug("heads = %r" % (heads,))
        return heads

    def known(self, nodes):
        assert isinstance(nodes, list)
        stream, _stats = self.edenapi.commitknown(self._reponame, nodes)
        knownnodes = set()
        # ex. [{'hgid': '11111111111111111111', 'known': {'Ok': False}}]
        for res in stream:
            node = res["hgid"]
            known = unwrap(res["known"], node)
            if known:
                knownnodes.add(node)
        shouldtrace = tracing.isenabled(tracing.LEVEL_TRACE)
        if shouldtrace:
            for node in sorted(nodes):
                tracing.trace("known %s: %s" % (hex(node), node in knownnodes))
        return [n in knownnodes for n in nodes]

    def listkeys(self, namespace):
        if namespace == "bookmarks":
            patterns = self.ui.configlist("remotenames", "selectivepulldefault")
        else:
            patterns = []
        return self.listkeyspatterns(namespace, patterns)

    def listkeyspatterns(self, namespace, patterns):
        result = util.sortdict()
        if namespace == "bookmarks":
            if not isinstance(patterns, list):
                patterns = sorted(patterns)
            # XXX: glob patterns are ignored.
            books, _stats = self.edenapi.bookmarks(self._reponame, patterns)
            for k, v in books.items():
                # ex. {'a': '3131313131313131313131313131313131313131', 'b': None}
                if v is not None:
                    result[k] = v
        tracing.debug("listkeyspatterns(%s, %r) = %r" % (namespace, patterns, result))
        return result

    def lookup(self, key):
        node = None
        if len(key) == 40:
            # hex node?
            try:
                node = bin(key)
            except Exception:
                pass
        if len(key) == 20:
            # binary node?
            node = key
        if node is not None:
            if self.known([node]) == [True]:
                return node
        # NOTE: Prefix match does not work yet.
        # bookmark?
        m = self.listkeyspatterns("bookmarks", [key])
        node = m.get(key, None)
        tracing.debug("lookup %s = %s" % (key, node and hex(node)))
        if node is None:
            raise error.RepoLookupError(_("unknown revision %r") % (key,))
        return node

    def pushkey(self, namespace, key, old, new):
        changed = False
        if namespace == "bookmarks":
            existing = hex(self.listkeyspatterns(namespace, [key]).get(key, b""))
            if old == existing:
                self._inner.setbookmark(key, bin(new))
                self._flush()
                changed = True
        tracing.debug(
            "pushkey %s %r: %r => %r (%s)"
            % (namespace, key, old, new, changed and "success" or "fail")
        )
        return changed

    def stream_out(self, shallow=False):
        raise NotImplementedError()

    def unbundle(self, bundle, heads, url):
        raise NotImplementedError()

    def iterbatch(self):
        return peer.localiterbatcher(self)


def unwrap(result, node=None):
    if "Ok" in result:
        return result["Ok"]
    elif "Err" in result:
        msg = _("server returned error: %r") % (result["Err"],)
    else:
        msg = _("server returned non-result: %r") % (result,)
    hint = None
    if node is not None:
        hint = _("for node %s") % (hex(node),)
    raise error.RepoError(msg, hint=hint)


instance = eagerpeer
