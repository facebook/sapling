# Copyright 2016-present Facebook. All Rights Reserved.
#
# protocol: logic for a server providing fastannotate support
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    error,
    extensions,
    hg,
    localrepo,
    wireproto,
)
from mercurial.i18n import _

import contextlib
import os

from . import context

# common

def _getmaster(ui):
    """get the mainbranch, and enforce it is set"""
    master = ui.config('fastannotate', 'mainbranch')
    if not master:
        raise error.Abort(_('fastannotate.mainbranch is required '
                            'for both the client and the server'))
    return master

# server-side

def _capabilities(orig, repo, proto):
    result = orig(repo, proto)
    result.append('getannotate')
    return result

def _getannotate(repo, proto, path, lastnode):
    # output:
    #   FILE := vfspath + '\0' + str(size) + '\0' + content
    #   OUTPUT := '' | FILE + OUTPUT
    result = ''
    with context.annotatecontext(repo, path) as actx:
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
                actx.rebuild() # delete files
        finally:
            # although the "with" context will also do a close/flush, we need
            # to do it early so we can send the correct respond to client.
            actx.close()
        # send back the full content of revmap and linelog, in the future we
        # may want to do some rsync-like fancy updating.
        # the lastnode check is not necessary if the client and the server
        # agree where the main branch is.
        if actx.lastnode != lastnode:
            for p in [actx.revmappath, actx.linelogpath]:
                if not os.path.exists(p):
                    continue
                content = ''
                with open(p, 'rb') as f:
                    content = f.read()
                vfsbaselen = len(repo.vfs.base + '/')
                relpath = p[vfsbaselen:]
                result += '%s\0%s\0%s' % (relpath, len(content), content)
    return result

def _registerwireprotocommand():
    if 'getannotate' in wireproto.commands:
        return
    wireproto.wireprotocommand('getannotate', 'path lastnode')(_getannotate)

def serveruisetup(ui):
    _registerwireprotocommand()
    extensions.wrapfunction(wireproto, '_capabilities', _capabilities)

# client-side

def _parseresponse(payload):
    result = {}
    i = 0
    l = len(payload) - 1
    state = 0 # 0: vfspath, 1: size
    vfspath = size = ''
    while i < l:
        ch = payload[i]
        if ch == '\0':
            if state == 1:
                result[vfspath] = buffer(payload, i + 1, int(size))
                i += int(size)
                state = 0
                vfspath = size = ''
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
            if not self.capable('getannotate'):
                ui.warn(_('remote peer cannot provide annotate cache\n'))
                yield None, None
            else:
                args = {'path': path, 'lastnode': lastnode or ''}
                f = wireproto.future()
                yield args, f
                yield _parseresponse(f.value)
    peer.__class__ = fastannotatepeer

@contextlib.contextmanager
def annotatepeer(repo):
    remotepath = repo.ui.expandpath(
        repo.ui.config('fastannotate', 'remotepath', 'default'))
    peer = hg.peer(repo.ui, {}, remotepath)
    try:
        yield peer
    finally:
        for i in ['close', 'cleanup']:
            getattr(peer, i, lambda: None)()

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
    batcher = peer.batch()
    ui.debug('fastannotate: requesting %d files\n' % len(paths))
    results = [batcher.getannotate(p, lastnodemap.get(p)) for p in paths]
    batcher.submit()

    ui.debug('fastannotate: server returned\n')
    for result in results:
        if result.value is None:
            continue
        for path, content in result.value.iteritems():
            # ignore malicious paths
            if not path.startswith('fastannotate/') or '/../' in (path + '/'):
                ui.debug('fastannotate: ignored malicious path %s\n' % path)
                continue
            if ui.debugflag:
                ui.debug('fastannotate: writing %d bytes to %s\n'
                         % (len(content), path))
            repo.vfs.makedirs(os.path.dirname(path))
            with repo.vfs(path, 'wb') as f:
                f.write(content)

def localreposetup(ui, repo):
    class fastannotaterepo(repo.__class__):
        def prefetchfastannotate(self, paths, peer=None):
            master = _getmaster(self.ui)
            needupdatepaths = []
            lastnodemap = {}
            for path in paths:
                with context.annotatecontext(self, path) as actx:
                    if not actx.isuptodate(master, strict=False):
                        needupdatepaths.append(path)
                        lastnodemap[path] = actx.lastnode
            if needupdatepaths:
                clientfetch(self, needupdatepaths, lastnodemap, peer)
    repo.__class__ = fastannotaterepo

def clientreposetup(ui, repo):
    _registerwireprotocommand()
    if isinstance(repo, localrepo.localrepository):
        localreposetup(ui, repo)
    if peersetup not in hg.wirepeersetupfuncs:
        hg.wirepeersetupfuncs.append(peersetup)
