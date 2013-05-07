# remotefilelog.py - extension for storing file contents remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient, remoterevlog
from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import ancestor, mdiff, parsers, error, util, dagutil, time
from mercurial import repair, extensions, filelog, revlog, wireproto, cmdutil
from mercurial import copies, traceback, store, context, changegroup, localrepo
from mercurial import commands, sshpeer, scmutil, dispatch, merge
import struct, zlib, errno, collections, time, os, pdb, socket, subprocess, lz4

shallowremote = False
localrepo.localrepository.supported.add('shallowrepo')

def uisetup(ui):
    entry = extensions.wrapcommand(commands.table, 'clone', cloneshallow)
    entry[1].append(('', 'shallow', None,
                     _("create a shallow clone which uses remote file history")))

def extsetup(ui):
    # the remote client communicates it's shallow capability via hello
    orig, args = wireproto.commands["hello"]
    def helloshallow(*args, **kwargs):
        global shallowremote
        shallowremote = True
        return orig(*args, **kwargs)
    wireproto.commands["hello_shallow"] = (helloshallow, args)

def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get('shallow'):
        addshallowcapability()
        def stream_in_shallow(orig, self, remote, requirements):
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self)

            requirements.add('shallowrepo')

            return orig(self, remote, requirements)
        wrapfunction(localrepo.localrepository, 'stream_in', stream_in_shallow)

    orig(ui, repo, *args, **opts)

def reposetup(ui, repo):
    if not repo.local():
        return

    isserverenabled = ui.configbool('remotefilelog', 'server')
    isshallowclient = "shallowrepo" in repo.requirements

    if isserverenabled and isshallowclient:
        raise Exception("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        # support file content requests
        wireproto.commands['getfiles'] = (getfiles, '')

    if isserverenabled or isshallowclient:
        # don't allow streaming clones from a shallow repo
        def stream(repo, proto):
            if isshallowclient:
                # don't allow cloning from a shallow repo since the cloned
                # repo would be unable to access local commits
                raise util.Abort(_("Cannot clone from a shallow repo."))

            return wireproto.stream(repo, proto)
        wireproto.commands['stream_out'] = (stream, '')

        # don't clone filelogs to shallow clients
        def _walkstreamfiles(orig, repo):
            if shallowremote:
                return repo.store.topfiles()
            return orig(repo)
        wrapfunction(wireproto, '_walkstreamfiles', _walkstreamfiles)

def setupclient(ui, repo):
    addshallowcapability();

def getfiles(repo, proto):
    """A server api for requesting particular versions of particular files.
    """
    def streamer():
        fin = proto.fin
        opener = repo.sopener
        while True:
            request = fin.readline()[:-1]
            if not request:
                break

            node = request[:40]
            path = request[40:]
            try:
                temprevlog = revlog.revlog(opener, "data/" + path + ".i")

                text = temprevlog.revision(bin(node))
                p1, p2 = temprevlog.parents(bin(node))
                text = lz4.compressHC(p1 + p2 + text)
            except Exception, ex:
                text = ""

            yield '%d\n%s' % (len(text), text)

            # it would be better to only flush after processing a whole batch
            # but currently we don't know if there are more requests coming
            proto.fout.flush()

    return wireproto.streamres(streamer())

def addshallowcapability():
    def callstream(orig, self, cmd, **args):
        if cmd == 'hello':
            cmd += '_shallow'
        return orig(self, cmd, **args)
    wrapfunction(sshpeer.sshpeer, '_callstream', callstream)
