# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# protocol: logic for a server providing fastannotate support

import contextlib
import os

from edenscm.mercurial import error, extensions, hg, localrepo, scmutil, wireproto
from edenscm.mercurial.i18n import _

from . import context


try:
    # pyre-fixme[18]: Global name `buffer` is undefined.
    buffer
except NameError:
    buffer = memoryview


# common


def _getmaster(ui):
    """get the mainbranch, and enforce it is set"""
    master = ui.config("fastannotate", "mainbranch")
    if not master:
        raise error.Abort(
            _(
                "fastannotate.mainbranch is required "
                "for both the client and the server"
            )
        )
    return master


# server-side


def _capabilities(orig, repo, proto):
    result = orig(repo, proto)
    result.append("getannotate")
    return result


def _getannotate(repo, proto, path, lastnode):
    # output:
    #   FILE := vfspath + '\0' + str(size) + '\0' + content
    #   OUTPUT := '' | FILE + OUTPUT
    result = ""
    buildondemand = repo.ui.configbool("fastannotate", "serverbuildondemand", True)
    with context.annotatecontext(repo, path) as actx:
        if buildondemand:
            # update before responding to the client
            master = _getmaster(repo.ui)
            try:
                if not actx.isuptodate(master):
                    actx.annotate(master, master)
            except Exception:
                # non-fast-forward move or corrupted. rebuild automically.
                actx.rebuild()
                try:
                    actx.annotate(master, master)
                except Exception:
                    actx.rebuild()  # delete files
            finally:
                # although the "with" context will also do a close/flush, we
                # need to do it early so we can send the correct respond to
                # client.
                actx.close()
        # send back the full content of revmap and linelog, in the future we
        # may want to do some rsync-like fancy updating.
        # the lastnode check is not necessary if the client and the server
        # agree where the main branch is.
        if actx.lastnode != lastnode:
            for p in [actx.revmappath, actx.linelogpath]:
                if not os.path.exists(p):
                    continue
                content = ""
                with open(p, "rb") as f:
                    content = f.read()
                vfsbaselen = len(repo.localvfs.base + "/")
                relpath = p[vfsbaselen:]
                result += "%s\0%s\0%s" % (relpath, len(content), content)
    return result


def _registerwireprotocommand():
    if "getannotate" in wireproto.commands:
        return
    wireproto.wireprotocommand("getannotate", "path lastnode")(_getannotate)


def serveruisetup(ui):
    _registerwireprotocommand()
    extensions.wrapfunction(wireproto, "_capabilities", _capabilities)


# client-side


def _parseresponse(payload):
    result = {}
    i = 0
    l = len(payload) - 1
    state = 0  # 0: vfspath, 1: size
    vfspath = size = ""
    while i < l:
        ch = payload[i]
        if ch == "\0":
            if state == 1:
                result[vfspath] = buffer(payload, i + 1, int(size))
                i += int(size)
                state = 0
                vfspath = size = ""
            elif state == 0:
                state = 1
        else:
            if state == 1:
                size += ch
            elif state == 0:
                vfspath += ch
        i += 1
    return result


def peersetup(ui, peer):
    class fastannotatepeer(peer.__class__):
        @wireproto.batchable
        def getannotate(self, path, lastnode=None):
            if not self.capable("getannotate"):
                ui.warn(_("remote peer cannot provide annotate cache\n"))
                yield None, None
            else:
                args = {"path": path, "lastnode": lastnode or ""}
                f = wireproto.future()
                yield args, f
                yield _parseresponse(f.value)

    peer.__class__ = fastannotatepeer


@contextlib.contextmanager
def annotatepeer(repo):
    ui = repo.ui

    # fileservice belongs to remotefilelog
    fileservice = getattr(repo, "fileservice", None)
    sharepeer = ui.configbool("fastannotate", "clientsharepeer", True)

    if sharepeer and fileservice:
        ui.debug("fastannotate: using remotefilelog connection pool\n")
        conn = repo.connectionpool.get(repo.fallbackpath)
        peer = conn.peer
        stolen = True
    else:
        remotepath = ui.expandpath(ui.config("fastannotate", "remotepath", "default"))
        peer = hg.peer(ui, {}, remotepath)
        stolen = False

    try:
        # Note: fastannotate requests should never trigger a remotefilelog
        # "getfiles" request, because "getfiles" puts the stream into a state
        # that does not exit. See "clientfetch": it does "getannotate" before
        # any hg stuff that could potentially trigger a "getfiles".
        yield peer
    finally:
        if not stolen:
            for i in ["close", "cleanup"]:
                getattr(peer, i, lambda: None)()
        else:
            conn.__exit__(None, None, None)


def clientfetch(repo, paths, lastnodemap=None, peer=None):
    """download annotate cache from the server for paths"""
    if not paths:
        return

    if peer is None:
        with annotatepeer(repo) as peer:
            return clientfetch(repo, paths, lastnodemap, peer)

    if lastnodemap is None:
        lastnodemap = {}

    ui = repo.ui
    batcher = peer.iterbatch()
    ui.debug("fastannotate: requesting %d files\n" % len(paths))
    for p in paths:
        batcher.getannotate(p, lastnodemap.get(p))
    # Note: This is the only place that fastannotate sends a request via SSH.
    # The SSH stream should not be in the remotefilelog "getfiles" loop.
    batcher.submit()
    results = list(batcher.results())

    ui.debug("fastannotate: server returned\n")
    for result in results:
        for path, content in result.iteritems():
            # ignore malicious paths
            if not path.startswith("fastannotate/") or "/../" in (path + "/"):
                ui.debug("fastannotate: ignored malicious path %s\n" % path)
                continue
            if ui.debugflag:
                ui.debug(
                    "fastannotate: writing %d bytes to %s\n" % (len(content), path)
                )
            repo.localvfs.makedirs(os.path.dirname(path))
            with repo.localvfs(path, "wb") as f:
                f.write(content)


def _filterfetchpaths(repo, paths):
    """return a subset of paths whose history is long and need to fetch linelog
    from the server. works with remotefilelog and non-remotefilelog repos.
    """
    threshold = repo.ui.configint("fastannotate", "clientfetchthreshold", 10)
    if threshold <= 0:
        return paths

    master = repo.ui.config("fastannotate", "mainbranch") or "default"

    if "remotefilelog" in repo.requirements:
        ctx = scmutil.revsingle(repo, master)
        f = lambda path: len(ctx[path].ancestormap())
    else:
        f = lambda path: len(repo.file(path))

    result = []
    for path in paths:
        try:
            if f(path) >= threshold:
                result.append(path)
        except Exception:  # file not found etc.
            result.append(path)

    return result


def localreposetup(ui, repo):
    class fastannotaterepo(repo.__class__):
        def prefetchfastannotate(self, paths, peer=None):
            master = _getmaster(self.ui)
            needupdatepaths = []
            lastnodemap = {}
            try:
                for path in _filterfetchpaths(self, paths):
                    with context.annotatecontext(self, path) as actx:
                        if not actx.isuptodate(master, strict=False):
                            needupdatepaths.append(path)
                            lastnodemap[path] = actx.lastnode
                if needupdatepaths:
                    clientfetch(self, needupdatepaths, lastnodemap, peer)
            except Exception as ex:
                # could be directory not writable or so, not fatal
                self.ui.debug("fastannotate: prefetch failed: %r\n" % ex)

    repo.__class__ = fastannotaterepo


def clientreposetup(ui, repo):
    _registerwireprotocommand()
    if isinstance(repo, localrepo.localrepository):
        localreposetup(ui, repo)
    if peersetup not in hg.wirepeersetupfuncs:
        hg.wirepeersetupfuncs.append(peersetup)
