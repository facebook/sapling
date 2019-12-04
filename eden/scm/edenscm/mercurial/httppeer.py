# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# httppeer.py - HTTP repository proxy classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import io
import os
import socket
import struct
import tempfile

from . import bundle2, error, httpconnection, pycompat, url, util, wireproto
from .i18n import _
from .node import nullid
from .pycompat import range


httplib = util.httplib
urlerr = util.urlerr
urlreq = util.urlreq


def encodevalueinheaders(value, header, limit):
    """Encode a string value into multiple HTTP headers.

    ``value`` will be encoded into 1 or more HTTP headers with the names
    ``header-<N>`` where ``<N>`` is an integer starting at 1. Each header
    name + value will be at most ``limit`` bytes long.

    Returns an iterable of 2-tuples consisting of header names and
    values as native strings.
    """
    # HTTP Headers are ASCII. Python 3 requires them to be unicodes,
    # not bytes. This function always takes bytes in as arguments.
    fmt = pycompat.strurl(header) + r"-%s"
    # Note: it is *NOT* a bug that the last bit here is a bytestring
    # and not a unicode: we're just getting the encoded length anyway,
    # and using an r-string to make it portable between Python 2 and 3
    # doesn't work because then the \r is a literal backslash-r
    # instead of a carriage return.
    valuelen = limit - len(fmt % r"000") - len(": \r\n")
    result = []

    n = 0
    for i in range(0, len(value), valuelen):
        n += 1
        result.append((fmt % str(n), pycompat.strurl(value[i : i + valuelen])))

    return result


def _wraphttpresponse(resp):
    """Wrap an HTTPResponse with common error handlers.

    This ensures that any I/O from any consumer raises the appropriate
    error and messaging.
    """
    origread = resp.read

    # pyre-fixme[11]: Annotation `__class__` is not defined as a type.
    class readerproxy(resp.__class__):
        def read(self, size=None):
            try:
                return origread(size)
            except httplib.IncompleteRead as e:
                # e.expected is an integer if length known or None otherwise.
                if e.expected:
                    msg = _(
                        "HTTP request error (incomplete response; "
                        "expected %d bytes got %d)"
                    ) % (e.expected, len(e.partial))
                else:
                    msg = _("HTTP request error (incomplete response)")

                raise error.PeerTransportError(
                    msg,
                    hint=_(
                        "this may be an intermittent network failure; "
                        "if the error persists, consider contacting the "
                        "network or server operator"
                    ),
                )
            except httplib.HTTPException as e:
                raise error.PeerTransportError(
                    _("HTTP request error (%s)") % e,
                    hint=_(
                        "this may be an intermittent network failure; "
                        "if the error persists, consider contacting the "
                        "network or server operator"
                    ),
                )

    resp.__class__ = readerproxy


class _multifile(object):
    def __init__(self, *fileobjs):
        for f in fileobjs:
            if not util.safehasattr(f, "length"):
                raise ValueError(
                    "_multifile only supports file objects that "
                    "have a length but this one does not:",
                    type(f),
                    f,
                )
        self._fileobjs = fileobjs
        self._index = 0

    @property
    def length(self):
        return sum(f.length for f in self._fileobjs)

    def read(self, amt=None):
        if amt <= 0:
            return "".join(f.read() for f in self._fileobjs)
        parts = []
        while amt and self._index < len(self._fileobjs):
            parts.append(self._fileobjs[self._index].read(amt))
            got = len(parts[-1])
            if got < amt:
                self._index += 1
            amt -= got
        return "".join(parts)

    def seek(self, offset, whence=os.SEEK_SET):
        if whence != os.SEEK_SET:
            raise NotImplementedError(
                "_multifile does not support anything other"
                " than os.SEEK_SET for whence on seek()"
            )
        if offset != 0:
            raise NotImplementedError(
                "_multifile only supports seeking to start, but that "
                "could be fixed if you need it"
            )
        for f in self._fileobjs:
            f.seek(0)
        self._index = 0


class httppeer(wireproto.wirepeer):
    def __init__(self, ui, path):
        self._path = path
        self._caps = None
        self._urlopener = None
        self._requestbuilder = None
        u = util.url(path)
        if u.query or u.fragment:
            raise error.Abort(
                _('unsupported URL component: "%s"') % (u.query or u.fragment)
            )

        # urllib cannot handle URLs with embedded user or passwd
        self._url, authinfo = u.authinfo()

        self._ui = ui
        ui.debug("using %s\n" % self._url)

        self._urlopener = url.opener(ui, authinfo)
        self._requestbuilder = urlreq.request

    def __del__(self):
        urlopener = getattr(self, "_urlopener", None)
        if urlopener:
            for h in urlopener.handlers:
                h.close()
                getattr(h, "close_all", lambda: None)()

    # Begin of _basepeer interface.

    @util.propertycache
    def ui(self):
        return self._ui

    def url(self):
        return self._path

    def local(self):
        return None

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        pass

    # End of _basepeer interface.

    # Begin of _basewirepeer interface.

    def capabilities(self):
        if self._caps is None:
            try:
                self._fetchcaps()
            except error.RepoError:
                self._caps = set()
            self.ui.debug("capabilities: %s\n" % (" ".join(self._caps or ["none"])))
        return self._caps

    # End of _basewirepeer interface.

    # look up capabilities only when needed

    def _fetchcaps(self):
        self._caps = set(self._call("capabilities").split())

    def _callstream(self, cmd, _compressible=False, **args):
        args = pycompat.byteskwargs(args)
        if cmd == "pushkey":
            args["data"] = ""
        data = args.pop("data", None)
        headers = args.pop("headers", {})

        self.ui.debug("sending %s command\n" % cmd)
        q = [("cmd", cmd)]
        headersize = 0
        varyheaders = []
        # Important: don't use self.capable() here or else you end up
        # with infinite recursion when trying to look up capabilities
        # for the first time.
        postargsok = self._caps is not None and "httppostargs" in self._caps
        if postargsok and args:
            strargs = urlreq.urlencode(sorted(args.items()))
            if not data:
                data = strargs
            else:
                if isinstance(data, bytes):
                    i = io.BytesIO(data)
                    i.length = len(data)
                    data = i
                argsio = io.BytesIO(strargs)
                argsio.length = len(strargs)
                data = _multifile(argsio, data)
            headers[r"X-HgArgs-Post"] = len(strargs)
        else:
            if len(args) > 0:
                httpheader = self.capable("httpheader")
                if httpheader:
                    headersize = int(httpheader.split(",", 1)[0])
            if headersize > 0:
                # The headers can typically carry more data than the URL.
                encargs = urlreq.urlencode(sorted(args.items()))
                for header, value in encodevalueinheaders(
                    encargs, "X-HgArg", headersize
                ):
                    headers[header] = value
                    varyheaders.append(header)
            else:
                q += sorted(args.items())
        qs = "?%s" % urlreq.urlencode(q)
        cu = "%s%s" % (self._url, qs)
        size = 0
        if util.safehasattr(data, "length"):
            size = data.length
        elif data is not None:
            size = len(data)
        if size and self.ui.configbool("ui", "usehttp2"):
            headers[r"Expect"] = r"100-Continue"
            headers[r"X-HgHttp2"] = r"1"
        if data is not None and r"Content-Type" not in headers:
            headers[r"Content-Type"] = r"application/mercurial-0.1"

        # Tell the server we accept application/mercurial-0.2 and multiple
        # compression formats if the server is capable of emitting those
        # payloads.
        protoparams = []

        mediatypes = set()
        if self._caps is not None:
            mt = self.capable("httpmediatype")
            if mt:
                protoparams.append("0.1")
                mediatypes = set(mt.split(","))

        if "0.2tx" in mediatypes:
            protoparams.append("0.2")

        if "0.2tx" in mediatypes and self.capable("compression"):
            # We /could/ compare supported compression formats and prune
            # non-mutually supported or error if nothing is mutually supported.
            # For now, send the full list to the server and have it error.
            comps = [
                e.wireprotosupport().name
                for e in util.compengines.supportedwireengines(util.CLIENTROLE)
            ]
            protoparams.append("comp=%s" % ",".join(comps))

        if protoparams:
            protoheaders = encodevalueinheaders(
                " ".join(protoparams), "X-HgProto", headersize or 1024
            )
            for header, value in protoheaders:
                headers[header] = value
                varyheaders.append(header)

        if varyheaders:
            headers[r"Vary"] = r",".join(varyheaders)

        req = self._requestbuilder(pycompat.strurl(cu), data, headers)

        if data is not None:
            self.ui.debug("sending %s bytes\n" % size)
            req.add_unredirected_header("Content-Length", "%d" % size)
        try:
            resp = self._urlopener.open(req)
        except urlerr.httperror as inst:
            if inst.code == 401:
                raise error.Abort(_("authorization failed"))
            raise
        except httplib.HTTPException as inst:
            self.ui.debug("http error while sending %s command\n" % cmd)
            self.ui.traceback()
            raise IOError(None, inst)

        # Insert error handlers for common I/O failures.
        _wraphttpresponse(resp)

        # record the url we got redirected to
        resp_url = pycompat.bytesurl(resp.geturl())
        if resp_url.endswith(qs):
            resp_url = resp_url[: -len(qs)]
        if self._url.rstrip("/") != resp_url.rstrip("/"):
            if not self.ui.quiet:
                self.ui.warn(_("real URL is %s\n") % resp_url)
        self._url = resp_url
        try:
            proto = pycompat.bytesurl(resp.getheader(r"content-type", r""))
        except AttributeError:
            proto = pycompat.bytesurl(resp.headers.get(r"content-type", r""))

        safeurl = util.hidepassword(self._url)
        if proto.startswith("application/hg-error"):
            raise error.OutOfBandError(resp.read())
        # accept old "text/plain" and "application/hg-changegroup" for now
        if not (
            proto.startswith("application/mercurial-")
            or (
                proto.startswith("text/plain")
                and not resp.headers.get("content-length")
            )
            or proto.startswith("application/hg-changegroup")
        ):
            self.ui.debug("requested URL: '%s'\n" % util.hidepassword(cu))
            raise error.RepoError(
                _(
                    "'%s' does not appear to be an hg repository:\n"
                    "---%%<--- (%s)\n%s\n---%%<---\n"
                )
                % (safeurl, proto or "no content-type", resp.read(1024))
            )

        if proto.startswith("application/mercurial-"):
            try:
                version = proto.split("-", 1)[1]
                version_info = tuple([int(n) for n in version.split(".")])
            except ValueError:
                raise error.RepoError(
                    _("'%s' sent a broken Content-Type " "header (%s)")
                    % (safeurl, proto)
                )

            # TODO consider switching to a decompression reader that uses
            # generators.
            if version_info == (0, 1):
                if _compressible:
                    return util.compengines["zlib"].decompressorreader(resp)
                return resp
            elif version_info == (0, 2):
                # application/mercurial-0.2 always identifies the compression
                # engine in the payload header.
                elen = struct.unpack("B", resp.read(1))[0]
                ename = resp.read(elen)
                engine = util.compengines.forwiretype(ename)
                return engine.decompressorreader(resp)
            else:
                raise error.RepoError(
                    _("'%s' uses newer protocol %s") % (safeurl, version)
                )

        if _compressible:
            return util.compengines["zlib"].decompressorreader(resp)

        return resp

    def _call(self, cmd, **args):
        fp = self._callstream(cmd, **args)
        try:
            return fp.read()
        finally:
            # if using keepalive, allow connection to be reused
            fp.close()

    def _callpush(self, cmd, cg, **args):
        # have to stream bundle to a temp file because we do not have
        # http 1.1 chunked transfer.

        types = self.capable("unbundle")
        try:
            types = types.split(",")
        except AttributeError:
            # servers older than d1b16a746db6 will send 'unbundle' as a
            # boolean capability. They only support headerless/uncompressed
            # bundles.
            types = [""]
        for x in types:
            if x in bundle2.bundletypes:
                type = x
                break

        tempname = bundle2.writebundle(self.ui, cg, None, type)
        fp = httpconnection.httpsendfile(self.ui, tempname, "rb")
        headers = {"Content-Type": "application/mercurial-0.1"}

        try:
            r = self._call(cmd, data=fp, headers=headers, **args)
            vals = r.split("\n", 1)
            if len(vals) < 2:
                raise error.ResponseError(_("unexpected response:"), r)
            return vals
        except socket.error as err:
            if err.args[0] in (errno.ECONNRESET, errno.EPIPE):
                raise error.Abort(_("push failed: %s") % err.args[1])
            raise error.Abort(err.args[1])
        finally:
            fp.close()
            os.unlink(tempname)

    def _calltwowaystream(self, cmd, fp, **args):
        fh = None
        fp_ = None
        filename = None
        try:
            # dump bundle to disk
            fd, filename = tempfile.mkstemp(prefix="hg-bundle-", suffix=".hg")
            fh = util.fdopen(fd, pycompat.sysstr("wb"))
            d = fp.read(4096)
            while d:
                fh.write(d)
                d = fp.read(4096)
            fh.close()
            # start http push
            fp_ = httpconnection.httpsendfile(self.ui, filename, "rb")
            headers = {"Content-Type": "application/mercurial-0.1"}
            return self._callstream(cmd, data=fp_, headers=headers, **args)
        finally:
            if fp_ is not None:
                fp_.close()
            if fh is not None:
                fh.close()
                os.unlink(filename)

    def _callcompressable(self, cmd, **args):
        return self._callstream(cmd, _compressible=True, **args)

    def _abort(self, exception):
        raise exception


class httpspeer(httppeer):
    def __init__(self, ui, path):
        if not url.has_https:
            raise error.Abort(_("Python support for SSL and HTTPS " "is not installed"))
        httppeer.__init__(self, ui, path)


def instance(ui, path, create):
    if create:
        raise error.Abort(_("cannot create new http repository"))
    if path.startswith("https:"):
        inst = httpspeer(ui, path)
    else:
        inst = httppeer(ui, path)
    try:
        # Try to do useful work when checking compatibility.
        # Usually saves a roundtrip since we want the caps anyway.
        inst._fetchcaps()
    except error.RepoError:
        # No luck, try older compatibility check.
        inst.between([(nullid, nullid)])
    return inst
