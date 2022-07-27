# Copyright (c) Meta Platforms, Inc. and affiliates.
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

import json
import os
import re as remod
import socket
import ssl
from enum import Enum
from struct import pack, unpack

from bindings import cats, clientinfo, zstd

from . import error, httpconnection, progress, sslutil, stdiopeer, util
from .i18n import _
from .pycompat import decodeutf8, encodeutf8, iswindows


if iswindows:
    # pyre-fixme[21]: Could not find a module corresponding to import `eden.thrift.windows_thrift`.
    from eden.thrift.windows_thrift import WindowsSocketHandle

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

    def __init__(self, ui, pipe, decompress=False):

        self._sendbuf = b""
        self._readbuf = b""
        self._readoffset = 0
        self._pipe = pipe
        self._ui = ui

        self._ui.log("mononokepeer", compression_enabled=decompress)

        if decompress:
            self._ui.debug("zstd compression on the wire is enabled\n")
            self._decompresser = zstd.zstream()
        else:
            self._decompresser = None

    def write(self, data):
        assert isinstance(data, bytes)
        self._sendbuf += data
        if self._sendbuf.endswith(b"\n"):
            self.flush()

        return len(data)

    def _read_segment(self):
        while True:
            r = b""
            while not r.endswith(NETSTRING_SEPARATOR):
                try:
                    buf = self._pipe.read(1)
                except Exception as e:
                    raise error.NetworkError("failed reading from pipe: {}".format(e))
                if not buf:
                    raise error.NetworkError("unexpected EOL, expected netstring digit")
                r += buf

            segmentlength = int(r[:-1])

            try:
                r = self._pipe.read(segmentlength + 1)
            except Exception as e:
                raise error.NetworkError("failed reading from pipe: {}".format(e))
            if len(r) != segmentlength + 1:
                raise error.NetworkError(
                    "unexpected read length, expected length {}, got length {}: '{}'".format(
                        segmentlength + 1, len(r), r
                    )
                )

            stdtype_raw, segment, ending = r[:1], r[1:-1], r[-1:]
            (stdtype,) = unpack("b", stdtype_raw)

            if ending != NETSTRING_ENDING:
                raise error.NetworkError(
                    "'%s' is not expected netencoding ending segment '%s'"
                    % (r[segmentlength], NETSTRING_ENDING)
                )

            if self._decompresser:
                segment = self._decompresser.decompress_buffer(segment)

            if stdtype == IoStream.STDOUT.value:
                return segment
            elif stdtype == IoStream.STDERR.value:
                stdiopeer._writestderror(self._ui, segment)
                continue
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

        self._ui.metrics.gauge("mononoke_read_bytes", size)
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


def maybestripsquarebrackets(hostname: str):
    """Strips the square braces from host name (if-present)

    socket.createconnection used for mononoke connections can't deal with ipv6
    addressed wrapped in square braces. This function allows us to support urls
    like mononoke://[::1]/repo

    util.url doesn't do it for us beacause it's not part of
    http://www.ietf.org/rfc/rfc2396.txt
    """
    bracketed_ipv6 = remod.compile(r"^\[(.*)\]$")
    m = bracketed_ipv6.match(hostname)
    if m is not None:
        return m.group(1)
    return hostname


class mononokepeer(stdiopeer.stdiopeer):
    def __init__(self, ui, path, create=False):
        super(mononokepeer, self).__init__(ui, path, create=create)
        self.sock = None

        u = util.url(path, parsequery=False, parsefragment=False)
        if u.scheme != "mononoke" or not u.host or u.path is None:
            self._abort(error.RepoError(_("couldn't parse location %s") % path))

        if u.passwd is not None:
            self._abort(error.RepoError(_("password in URL not supported")))

        self._clientinfo = clientinfo.clientinfo(ui._uiconfig._rcfg)
        self._user = u.user
        self._host = maybestripsquarebrackets(u.host)
        self._port = u.port or 443
        self._path = u.path
        self._compression = ui.configwith(bool, "mononokepeer", "compression")
        self._sockettimeout = ui.configwith(float, "mononokepeer", "sockettimeout")
        self._unix_socket_proxy = ui.config("auth_proxy", "unix_socket_path")
        self._auth_proxy_http = ui.config("auth_proxy", "http_proxy")
        self._confheaders = ui.config("http", "extra_headers_json")
        self._verbose = ui.configwith(bool, "http", "verbose")
        try:
            self._cats = cats.getcats(ui._uiconfig._rcfg, raise_if_missing=True)
        except Exception as e:
            ui.log("features", feature="missing-cats")
            ui.debug("CATs missing: %s. Identities won't be propagated.\n" % e)
            self._cats = None

        if self._auth_proxy_http:
            u = util.url(self._auth_proxy_http, parsequery=False, parsefragment=False)
            self._auth_proxy_http_host = u.host
            self._auth_proxy_http_port = u.port

        if not (self._unix_socket_proxy or self._auth_proxy_http):
            authdata = httpconnection.readauthforuri(self._ui, path, self._user)
            if not authdata:
                errormessage = _(
                    "No certificates have been found to connect to Mononoke"
                )
                self._abort(error.CertificateError(errormessage))

            (authname, auth) = authdata
            self._cert = auth.get("cert")
            self._key = auth.get("key")
            self._cn = auth.get("cn") or self._host

        if create:
            self._abort(
                error.RepoError(_("creating repositories in Mononoke is not supported"))
            )

        with self.ui.timeblockedsection(
            "mononokesetup"
        ), progress.suspend(), util.traced("mononoke_setup", cat="blocked"):
            self._validaterepo()

    def _connectionerror(self, ex, tlserror=False):
        msg = ""

        msg += _("failed to connect to ")
        msg += "%s:%s\n" % (self._host, self._port)
        msg += " reason: %s\n" % ex
        if self._unix_socket_proxy:
            msg += " UDS proxy:  %s\n" % self._unix_socket_proxy
        elif self._auth_proxy_http:
            msg += " HTTP proxy:  %s\n" % self._auth_proxy_http
        else:
            msg += " cn:     %s\n" % self._cn
            msg += " cert:   %s\n" % self._cert
            msg += " key:    %s\n" % self._key

        if tlserror or isinstance(ex, ssl.SSLError):
            msg += "\n"
            msg += self.ui.config("help", "tlsauthhelp") or ""

        self._abort(error.BadResponseError(msg))

    def _setmononokesock(self):
        with self.ui.timeblockedsection("mononoke_tcp"):
            try:
                if self._unix_socket_proxy:
                    if iswindows:
                        self.sock = WindowsSocketHandle()
                    else:
                        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    self.sock.settimeout(self._sockettimeout)
                    self.sock.connect(self._unix_socket_proxy)
                    return
                elif self._auth_proxy_http:
                    self.sock = socket.create_connection(
                        (self._auth_proxy_http_host, self._auth_proxy_http_port),
                        self._sockettimeout,
                    )
                    return
                else:
                    self.sock = socket.create_connection(
                        (self._host, self._port), self._sockettimeout
                    )
            except IOError as ex:
                self._connectionerror(ex)

        with self.ui.timeblockedsection("mononoke_tls"):
            try:
                self.sock = sslutil.wrapsocket(
                    self.sock,
                    self._key,
                    self._cert,
                    ui=self.ui,
                    serverhostname=self._cn,
                )
                sslutil.validatesocket(self.sock)

            except IOError as ex:
                self._connectionerror(ex, tlserror=True)

    def _validaterepo(self):
        # cleanup up previous run
        self._cleanup()
        decompress = False

        self._setmononokesock()

        with self.ui.timeblockedsection("mononoke_headers"):
            try:
                requestline = encodeutf8("GET /{} HTTP/1.1\r\n".format(self._path))
                proxy = (
                    "+proxied"
                    if self._unix_socket_proxy or self._auth_proxy_http
                    else ""
                )
                useragent = "mercurial/mononoke-peer{}".format(proxy)

                headers = {
                    "Host": self._host,
                    "User-Agent": useragent,
                    "Connection": "Upgrade",
                    "Upgrade": "websocket",
                }
                headers["X-Client-Info"] = self._clientinfo.into_json().decode()

                if self._cats:
                    headers["x-forwarded-cats"] = self._cats

                if self._compression:
                    headers["X-Client-Compression"] = "zstd=stdin"

                if os.getenv("CLIENT_DEBUG"):
                    headers["X-Client-Debug"] = "true"

                if self._confheaders:
                    headers.update(json.loads(self._confheaders))

                headersstr = b"\r\n".join(
                    map(lambda x: encodeutf8(x[0] + ": " + x[1]), headers.items())
                )

                httprequest = requestline + headersstr + b"\r\n\r\n"

                if self._verbose:
                    self.ui.status(httprequest.decode())

                self.sock.send(httprequest)

                if iswindows:
                    self.handle = socket.socket.makefile(self.sock, mode="rwb")
                else:
                    # Read HTTP response headers so we can start our own
                    # protocol afterwards
                    self.handle = self.sock.makefile(mode="rwb")

                # First line is wether request was successful
                line = self.handle.readline(1024).strip()

                if self._verbose:
                    self.ui.status("< {}".format(line))

                headerparts = line.split()
                if len(headerparts) < 2:
                    self._abort(
                        error.BadResponseError("invalid http response: %s" % line)
                    )

                if headerparts[0] != b"HTTP/1.1":
                    self._abort(
                        error.BadResponseError(
                            'unexpected server response: "{}"'.format(decodeutf8(line))
                        )
                    )

                httpcode = headerparts[1]
                httpstatus = b" ".join(headerparts[2:])
                bodylength = 0
                apeadvice = b""
                x2pagentderrortype = b""
                x2pagentderrormsg = b""

                # Strip away all headers so we can start decoding Mercurial
                # wire protocol or read HTTP response body
                while line:
                    line = self.handle.readline(1024).strip()
                    if self._verbose:
                        print("< {}".format(line))
                    if line.lower().startswith(b"x-mononoke-encoding:"):
                        decompress = True
                    elif line.lower().startswith(b"content-length:"):
                        bodylength = int(line.split(b" ", 1)[1])
                    elif line.lower().startswith(b"x-fb-validated-x2pauth-advice"):
                        apeadvice = line.split(b" ", 1)[1]
                    elif line.lower().startswith(b"x-x2pagentd-error-type:"):
                        x2pagentderrortype = line.split(b" ", 1)[1]
                    elif line.lower().startswith(b"x-x2pagentd-error-msg:"):
                        x2pagentderrormsg = line.split(b" ", 1)[1]

                if httpcode != b"101":
                    bodyerrmsg = self.handle.read(bodylength)
                    x2pagentderr = (
                        "x2pagentd: {}. {}".format(
                            decodeutf8(x2pagentderrortype),
                            decodeutf8(x2pagentderrormsg),
                        )
                        if x2pagentderrortype
                        else ""
                    )
                    self._abort(
                        error.BadResponseError(
                            'unexpected server response: "{} {}": {}\n{}\n{}'.format(
                                decodeutf8(httpcode),
                                decodeutf8(httpstatus),
                                decodeutf8(bodyerrmsg),
                                decodeutf8(apeadvice),
                                x2pagentderr,
                            )
                        )
                    )

            except IOError as ex:
                self._connectionerror(ex)

            self._pipeo = self._pipei = mononokepipe(self.ui, self.handle, decompress)

        self.ui.metrics.gauge("mononoke_connections")

        def badresponse(errortext):
            msg = _("no suitable response from mononoke")
            if errortext:
                msg += ": '%s'" % errortext
            self._abort(error.BadResponseError(msg))

        with self.ui.timeblockedsection("mononoke_hello"):
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
