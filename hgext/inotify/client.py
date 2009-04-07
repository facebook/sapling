# client.py - inotify status client
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
# Copyright 2009 Nicolas Dumazet <nicdumz@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from mercurial.i18n import _
import common
import os, socket, struct

class client(object):
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.sock = socket.socket(socket.AF_UNIX)

    def _connect(self):
        sockpath = self.repo.join('inotify.sock')
        try:
            self.sock.connect(sockpath)
        except socket.error, err:
            if err[0] == "AF_UNIX path too long":
                sockpath = os.readlink(sockpath)
                self.sock.connect(sockpath)
            else:
                raise

    def _send(self, data):
        """Sends protocol version number, and the data"""
        self.sock.sendall(chr(common.version) + data)

        self.sock.shutdown(socket.SHUT_WR)

    def _receive(self):
        """
        Read data, check version number, extract headers,
        and returns a tuple (data descriptor, header)
        Returns (None, None) on error
        """
        cs = common.recvcs(self.sock)
        version = ord(cs.read(1))
        if version != common.version:
            self.ui.warn(_('(inotify: received response from incompatible '
                      'server version %d)\n') % version)
            return None, None

        # only one type of request is supported for now
        type = 'STAT'
        hdrfmt = common.resphdrfmts[type]
        hdrsize = common.resphdrsizes[type]
        try:
            resphdr = struct.unpack(hdrfmt, cs.read(hdrsize))
        except struct.error:
            return None, None

        return cs, resphdr

    def query(self, req):
        self._connect()

        self._send(req)

        return self._receive()

    def statusquery(self, names, match, ignored, clean, unknown=True):

        def genquery():
            for n in names:
                yield n
            states = 'almrx!'
            if ignored:
                raise ValueError('this is insanity')
            if clean: states += 'c'
            if unknown: states += '?'
            yield states

        req = '\0'.join(genquery())

        cs, resphdr = self.query(req)

        if not cs:
            return None

        def readnames(nbytes):
            if nbytes:
                names = cs.read(nbytes)
                if names:
                    return filter(match, names.split('\0'))
            return []

        return map(readnames, resphdr)
