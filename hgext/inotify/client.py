# client.py - inotify status client
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import gettext as _
from mercurial import ui
import common
import os, select, socket, stat, struct, sys

def query(ui, repo, names, match, list_ignored, list_clean, list_unknown=True):
    sock = socket.socket(socket.AF_UNIX)
    sockpath = repo.join('inotify.sock')
    sock.connect(sockpath)

    def genquery():
        for n in names or []:
            yield n
        states = 'almrx!'
        if list_ignored:
            raise ValueError('this is insanity')
        if list_clean: states += 'n'
        if list_unknown: states += '?'
        yield states

    req = '\0'.join(genquery())

    sock.sendall(chr(common.version))
    sock.sendall(req)
    sock.shutdown(socket.SHUT_WR)

    cs = common.recvcs(sock)
    version = ord(cs.read(1))

    if version != common.version:
        ui.warn(_('(inotify: received response from incompatible server '
                  'version %d)\n') % version)
        return None

    try:
        resphdr = struct.unpack(common.resphdrfmt, cs.read(common.resphdrsize))
    except struct.error:
        return None

    def readnames(nbytes):
        if nbytes:
            names = cs.read(nbytes)
            if names:
                return filter(match, names.split('\0'))
        return []

    return map(readnames, resphdr)
