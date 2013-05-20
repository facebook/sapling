# fileserverclient.py - client for communicating with the file server
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial import util
import os, socket, lz4, time

# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchedbytes = 0
contentbytes = 0
metadatabytes = 0

_downloading = _('downloading')

client = None

def getcachekey(file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(pathhash, id)

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
            pathhash = util.sha1(file).hexdigest()
            request += "%s%s%s\1" % (id, pathhash, file)

        self.socket.sendall(request)

        missing = []
        total = count
        self.ui.progress(_downloading, 0, total=count)

        global fetchedbytes
        global metadatabytes
        global contentbytes

        while count > 0:
            count -= 1
            raw = self.readuntil()

            if not raw:
                raise util.Abort(_("error downloading file contents: " +
                                   "connection closed early"))

            id = raw[:40]
            pathhash = raw[40:80]
            size = int(raw[80:])

            fetchedbytes += len(raw) + size + 1

            if size == 0:
                missing.append(id)
                continue

            data = self.read(size)
            data = lz4.decompress(data)

            index = data.index('\0')
            contentsize = int(data[:index])

            contentbytes += contentsize
            metadatabytes += len(data) - contentsize

            filecachepath = os.path.join(self.cachepath, pathhash)
            if not os.path.exists(filecachepath):
                os.makedirs(filecachepath)

            idcachepath = os.path.join(filecachepath, id)

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
        if fetches:
            print ("%s fetched over %d fetches - %0.2f MB (%0.2f MB content / %0.2f MB metadata) " +
                  "over %0.2fs = %0.2f MB/s") % (
                    fetched,
                    fetches,
                    float(fetchedbytes) / 1024 / 1024,
                    float(contentbytes) / 1024 / 1024,
                    float(metadatabytes) / 1024 / 1024,
                    fetchcost,
                    float(fetchedbytes) / 1024 / 1024 / max(0.001, fetchcost))

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
            new = self.socket.recv(4096)
            if not new:
                raise util.Abort(_("Connection closed early"))
            self.buffer += new
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

            key = getcachekey(file, id)
            idcachepath = os.path.join(self.cachepath, key)
            idlocalpath = os.path.join(storepath, 'data', key)
            if os.path.exists(idcachepath) or os.path.exists(idlocalpath):
                continue

            missingids.append((file, id))

        if missingids:
            global fetches, fetched, fetchcost
            fetches += 1
            fetched += len(missingids)
            start = time.time()
            missingids = self.request(missingids)
            if missingids:
                raise util.Abort(_("unable to download %d files") % len(missingids))
            fetchcost += time.time() - start
