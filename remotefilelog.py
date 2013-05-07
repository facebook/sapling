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

def setupclient(ui, repo):
    pass

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
