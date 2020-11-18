# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# mononokepeer.py - mononoke protocol class for mercurial
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Mononoke wraps the Mercurial SSH Protocol inside a TLS Socket to be able
# to encode stdin, stderr and stdout inside the socket. We prefix the protocol
# with an HTTP handshake using HTTP 101 to switch protocols to be able to go
# through load balancing proxies.
#
# The wrapping protocol first prefixes a buffer with a single byte indicating
# wether this is intended for stdin, stderr or stdout and then wrapping the
# buffer inside a netstring encoding so that sender/receiver knows the bounds.
#
# More information on Netstring can be found at:
#   https://en.wikipedia.org/wiki/Netstring
#
# Take this sample message where Mercurial starts the handshake over stdin
# with:
#   <ssh stdin channel> hello world\n
#
# For Mononoke this would be send over the TLS socket as:
#   <tls socket> 7:\x00hello\n,
# or as hexadecimals:
#   37 3a 00 68 65 6c 6c 6f 0a 2c

from __future__ import absolute_import

import os
import socket
from enum import Enum
from struct import pack, unpack

from . import error, progress, sslutil, util, stdiopeer
from .i18n import _
from .pycompat import decodeutf8, encodeutf8

# Netencoding special characters
NETSTRING_SEPARATOR = b":"
NETSTRING_ENDING = b","


# Matches IoStream in Mononoke
class IoStream(Enum):
    STDIN = 0
    STDOUT = 1
    STDERR = 2


class mononokepipe(object):
    """Wraps a pipe that count the number of bytes read/written to it"""

    def __init__(self, ui, pipe):
        self._sendbuf = b""
        self._readbuf = b""
        self._readoffset = 0
        self._ui = ui
        self._pipe = pipe
        self._totalbytes = 0

    def write(self, data):
        assert isinstance(data, bytes)
        self._sendbuf += data
        if self._sendbuf.endswith(b"\n"):
            self.flush()

        return len(data)

    def _read_segment(self):
        r = b""
        while not r.endswith(NETSTRING_SEPARATOR):
            buf = self._pipe.read(1)
            if not buf:
                raise error.Abort("unexpected EOL, expected netstring digit")
            r += buf

        segmentlength = int(r[:-1])
        r = self._pipe.read(segmentlength + 1)
        if len(r) != segmentlength + 1:
            raise error.Abort(
                "unexpected read length, expected length {}, got length {}: '{}'".format(
                    segmentlength + 1, len(r), r
                )
            )

        stdtype_raw, segment, ending = r[:1], r[1:-1], r[-1:]
        (stdtype,) = unpack("b", stdtype_raw)

        if ending != NETSTRING_ENDING:
            raise error.Abort(
                "'%s' is not expected netencoding ending segment '%s'"
                % (r[segmentlength], NETSTRING_ENDING)
            )

        if stdtype == IoStream.STDOUT.value:
            return segment
        elif stdtype == IoStream.STDERR.value:
            stdiopeer._writestderror(self._ui, segment)
            return self._read_segment()
        else:
            raise error.Abort("unexpected stdtype '{}'".format(stdtype))

    def read(self, size):
        bufs = [self._readbuf[self._readoffset :]]
        availlength = len(bufs[-1])
        while availlength < size:
            self._readbuf = self._read_segment()
            self._readoffset = 0
            availlength += len(self._readbuf)
            bufs.append(self._readbuf)

        buf = b"".join(bufs)
        if availlength > size:
            self._readoffset = len(self._readbuf) - (availlength - size)
        else:
            self._reset_read_buf()

        return buf[:size]

    def readline(self):
        bufs = [self._readbuf[self._readoffset :]]
        while b"\n" not in bufs[-1]:
            self._readbuf = self._read_segment()

            # We've reached EOF, just abort without
            # finding newline
            if not self._readbuf:
                self._reset_read_buf()
                return b"".join(bufs)

            bufs.append(self._readbuf)

        # include \n in the part being cut off
        buf = b"".join(bufs)
        offset = buf.find(b"\n") + 1
        r = buf[:offset]
        if len(r) < len(buf):
            self._readoffset = len(self._readbuf) - (len(buf) - offset) + 1
        else:
            self._reset_read_buf()

        self._ui.metrics.gauge("mononoke_read_bytes", len(r))
        return r

    def _reset_read_buf(self):
        self._readbuf = b""
        self._readoffset = 0

    def close(self):
        return self._pipe.close()

    def flush(self):
        # Get and reset buffer
        data = self._sendbuf
        self._sendbuf = b""

        iostream = pack("b", IoStream.STDIN.value)
        netstringlength = len(data) + 1
        netstringprefix = encodeutf8(str(netstringlength)) + NETSTRING_SEPARATOR

        self._pipe.write(netstringprefix)
        self._pipe.write(iostream)
        self._pipe.write(data)
        self._pipe.write(NETSTRING_ENDING)

        self._ui.metrics.gauge(
            "mononoke_write_bytes",
            len(netstringprefix) + len(iostream) + len(data) + len(NETSTRING_ENDING),
        )

        return self._pipe.flush()


class mononokepeer(stdiopeer.stdiopeer):
    def __init__(self, ui, path, create=False):
        super(mononokepeer, self).__init__(ui, path, create=create)
        self.sock = None

        u = util.url(path, parsequery=False, parsefragment=False)
        if u.scheme != "mononoke" or not u.host or u.path is None:
            self._abort(error.RepoError(_("couldn't parse location %s") % path))

        if u.passwd is not None:
            self._abort(error.RepoError(_("password in URL not supported")))

        self._user = u.user
        self._host = u.host
        self._port = u.port or 443
        self._path = u.path
        self._cn = ui.config("mononokepeer", "cn") or self._host

        # Let's share certificate finding logic with EdenAPI
        self._cert = ui.config("auth", "edenapi.cert")
        self._key = ui.config("auth", "edenapi.key")

        if self._cert is None or self._key is None:
            self._abort(error.RepoError(_("missing certificate or private key")))

        if create:
            self._abort(
                error.RepoError(_("creating repositories in Mononoke is not supported"))
            )

        with self.ui.timeblockedsection(
            "mononokesetup"
        ), progress.suspend(), util.traced("mononoke_setup", cat="blocked"):
            self._validaterepo()

    def _validaterepo(self):
        # cleanup up previous run
        self._cleanup()

        try:
            self.sock = socket.create_connection((self._host, self._port))
            self.sock = sslutil.wrapsocket(
                self.sock,
                self._key,
                self._cert,
                ui=self.ui,
                serverhostname=self._cn,
            )
            sslutil.validatesocket(self.sock)

            headers = [
                encodeutf8("GET /{} HTTP/1.1".format(self._path)),
                encodeutf8("Host: {}".format(self._host)),
                b"User-Agent: mercurial/mononoke-peer",
                b"Connection: Upgrade",
                b"Upgrade: mercurial/v1",
            ]

            if os.getenv("CLIENT_DEBUG"):
                headers.append(b"X-Client-Debug: true")

            self.sock.send(b"\r\n".join(headers) + b"\r\n\r\n")

            # Read HTTP response headers so we can start our own
            # protocol afterwards
            self.handle = self.sock.makefile(mode="rwb")

            # First line is wether request was successful
            line = self.handle.readline(1024).strip()
            headerparts = line.split()
            if len(headerparts) < 2:
                self._abort(error.BadResponseError("invalid http response: %s" % line))

            if headerparts[0] != b"HTTP/1.1" and headerparts[1] != b"101":
                self._abort(
                    error.BadResponseError(
                        "expected HTTP/1.1 101 Upgrade, got: {}".format(headerparts)
                    )
                )

            # Strip away all headers so we can start decoding Mercurial
            # wire protocol
            while line:
                line = self.handle.readline(1024).strip()

        except IOError as ex:
            msg = _("failed to connect to ")
            msg += "%s:%s\n" % (self._host, self._port)
            msg += " reason: %s\n" % ex
            msg += " cn:     %s\n" % self._cn
            msg += " cert:   %s\n" % self._cert
            msg += " key:    %s\n" % self._key
            self._abort(error.BadResponseError(msg))

        self._pipei = mononokepipe(self.ui, self.handle)
        self._pipeo = mononokepipe(self.ui, self.handle)

        self.ui.metrics.gauge("mononoke_connections")

        def badresponse(errortext):
            msg = _("no suitable response from mononoke")
            if errortext:
                msg += ": '%s'" % errortext
            self._abort(error.BadResponseError(msg))

        try:
            reader = self._callstream("hello")

            # availlength of capabilities line, safe to skip
            reader.readline()

            # actual capabilities, which we should parse
            l = decodeutf8(reader.readline())
            if not l.startswith("capabilities:"):
                self._abort(
                    error.BadResponseError("no capabilities advertised by mononoke")
                )
            self._caps.update(l[:-1].split(":")[1].split())
        except IOError as ex:
            badresponse(str(ex))

    def _cleanup(self):
        self._pipeo = self._pipei = None

        if self.sock is not None:
            self.sock.close()
            self.sock = None

    __del__ = _cleanup


instance = mononokepeer
