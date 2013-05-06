# fileserverclient.py - client for communicating with the file server
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial import util
import os, socket, lz4

_downloading = _('downloading')

client = None

class fileserverclient(object):
    """A client for requesting files from the remote file server.
    """
    def __init__(self, ui):
        self.ui = ui
        self.socket = None
        self.buffer = ''
        self.server = ui.config("remotefilelog", "serveraddress")
        self.port = ui.configint("remotefilelog", "serverport")
        self.cachepath = ui.config("remotefilelog", "cachepath")

        if not os.path.exists(self.cachepath):
            os.makedirs(self.cachepath)

    def request(self, fileids):
        """Takes a list of filename/node pairs and fetches them from the
        server. Files are stored in the self.cachepath.
        A list of nodes that the server couldn't find is returned.
        If the connection fails, an exception is raised.
        """

        if not self.socket:
            self.connect()

        count = len(fileids)
        request = "%d\1" % count
        for file, id in fileids:
            request += "%s%s\1" % (id, file)

        self.socket.sendall(request)

        missing = []
        total = count
        self.ui.progress(_downloading, 0, total=count)

        while count > 0:
            count -= 1
            raw = self.readuntil()

            if not raw:
                raise util.Abort(_("error downloading file contents: " +
                                   "connection closed early"))

            id = raw[:40]
            size = int(raw[40:])

            if size == 0:
                missing.append(id)
                continue

            data = self.read(size)
            data = lz4.decompress(data)

            idcachepath = os.path.join(self.cachepath, id)
            f = open(idcachepath, "w")
            try:
                f.write(data)
            finally:
                f.close()

            self.ui.progress(_downloading, total - count, total=total)

        self.ui.progress(_downloading, None)

        return missing

    def connect(self):
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.socket.connect((self.server, self.port))

    def close(self):
        if self.socket:
            self.socket.sendall("\1\1")
            self.socket.close()
            self.socket = None

    def read(self, size):
        while len(self.buffer) < size:
            self.buffer += self.socket.recv(size)

        result = self.buffer[:size]
        self.buffer = self.buffer[size:]
        return result

    def readuntil(self, delimiter="\1"):
        index = self.buffer.find(delimiter)
        while index == -1:
            self.buffer += self.socket.recv(4096)
            index = self.buffer.find(delimiter)

        result = self.buffer[:index]
        self.buffer = self.buffer[(index + 1):]
        return result

    def prefetch(self, storepath, fileids):
        """downloads the given file versions to the cache
        """
        missingids = []
        for file, id in fileids:
            # hack
            if file == '.hgtags':
                continue

            idcachepath = os.path.join(self.cachepath, id)
            idlocalpath = os.path.join(storepath, 'localdata', id)
            if os.path.exists(idcachepath) or os.path.exists(idlocalpath):
                continue

            missingids.append((file, id))

        if missingids:
            missingids = self.request(missingids)
            if missingids:
                raise util.Abort(_("unable to download %d files") % len(missingids))
