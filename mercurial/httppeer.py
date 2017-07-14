# httppeer.py - HTTP repository proxy classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import socket
import struct
import tempfile

from .i18n import _
from .node import nullid
from . import (
    bundle2,
    error,
    httpconnection,
    pycompat,
    statichttprepo,
    url,
    util,
    wireproto,
)

httplib = util.httplib
urlerr = util.urlerr
urlreq = util.urlreq

def encodevalueinheaders(value, header, limit):
    """Encode a string value into multiple HTTP headers.

    ``value`` will be encoded into 1 or more HTTP headers with the names
    ``header-<N>`` where ``<N>`` is an integer starting at 1. Each header
    name + value will be at most ``limit`` bytes long.

    Returns an iterable of 2-tuples consisting of header names and values.
    """
    fmt = header + '-%s'
    valuelen = limit - len(fmt % '000') - len(': \r\n')
    result = []

    n = 0
    for i in xrange(0, len(value), valuelen):
        n += 1
        result.append((fmt % str(n), value[i:i + valuelen]))

    return result

def _wraphttpresponse(resp):
    """Wrap an HTTPResponse with common error handlers.

    This ensures that any I/O from any consumer raises the appropriate
    error and messaging.
    """
    origread = resp.read

    class readerproxy(resp.__class__):
        def read(self, size=None):
            try:
                return origread(size)
            except httplib.IncompleteRead as e:
                # e.expected is an integer if length known or None otherwise.
                if e.expected:
                    msg = _('HTTP request error (incomplete response; '
                            'expected %d bytes got %d)') % (e.expected,
                                                           len(e.partial))
                else:
                    msg = _('HTTP request error (incomplete response)')

                raise error.PeerTransportError(
                    msg,
                    hint=_('this may be an intermittent network failure; '
                           'if the error persists, consider contacting the '
                           'network or server operator'))
            except httplib.HTTPException as e:
                raise error.PeerTransportError(
                    _('HTTP request error (%s)') % e,
                    hint=_('this may be an intermittent network failure; '
                           'if the error persists, consider contacting the '
                           'network or server operator'))

    resp.__class__ = readerproxy

class httppeer(wireproto.wirepeer):
    def __init__(self, ui, path):
        self.path = path
        self.caps = None
        self.handler = None
        self.urlopener = None
        self.requestbuilder = None
        u = util.url(path)
        if u.query or u.fragment:
            raise error.Abort(_('unsupported URL component: "%s"') %
                             (u.query or u.fragment))

        # urllib cannot handle URLs with embedded user or passwd
        self._url, authinfo = u.authinfo()

        self.ui = ui
        self.ui.debug('using %s\n' % self._url)

        self.urlopener = url.opener(ui, authinfo)
        self.requestbuilder = urlreq.request

    def __del__(self):
        urlopener = getattr(self, 'urlopener', None)
        if urlopener:
            for h in urlopener.handlers:
                h.close()
                getattr(h, "close_all", lambda : None)()

    def url(self):
        return self.path

    # look up capabilities only when needed

    def _fetchcaps(self):
        self.caps = set(self._call('capabilities').split())

    def _capabilities(self):
        if self.caps is None:
            try:
                self._fetchcaps()
            except error.RepoError:
                self.caps = set()
            self.ui.debug('capabilities: %s\n' %
                          (' '.join(self.caps or ['none'])))
        return self.caps

    def lock(self):
        raise error.Abort(_('operation not supported over http'))

    def _callstream(self, cmd, _compressible=False, **args):
        if cmd == 'pushkey':
            args['data'] = ''
        data = args.pop('data', None)
        headers = args.pop('headers', {})

        self.ui.debug("sending %s command\n" % cmd)
        q = [('cmd', cmd)]
        headersize = 0
        varyheaders = []
        # Important: don't use self.capable() here or else you end up
        # with infinite recursion when trying to look up capabilities
        # for the first time.
        postargsok = self.caps is not None and 'httppostargs' in self.caps
        # TODO: support for httppostargs when data is a file-like
        # object rather than a basestring
        canmungedata = not data or isinstance(data, basestring)
        if postargsok and canmungedata:
            strargs = urlreq.urlencode(sorted(args.items()))
            if strargs:
                if not data:
                    data = strargs
                elif isinstance(data, basestring):
                    data = strargs + data
                headers['X-HgArgs-Post'] = len(strargs)
        else:
            if len(args) > 0:
                httpheader = self.capable('httpheader')
                if httpheader:
                    headersize = int(httpheader.split(',', 1)[0])
            if headersize > 0:
                # The headers can typically carry more data than the URL.
                encargs = urlreq.urlencode(sorted(args.items()))
                for header, value in encodevalueinheaders(encargs, 'X-HgArg',
                                                          headersize):
                    headers[header] = value
                    varyheaders.append(header)
            else:
                q += sorted(args.items())
        qs = '?%s' % urlreq.urlencode(q)
        cu = "%s%s" % (self._url, qs)
        size = 0
        if util.safehasattr(data, 'length'):
            size = data.length
        elif data is not None:
            size = len(data)
        if size and self.ui.configbool('ui', 'usehttp2'):
            headers['Expect'] = '100-Continue'
            headers['X-HgHttp2'] = '1'
        if data is not None and 'Content-Type' not in headers:
            headers['Content-Type'] = 'application/mercurial-0.1'

        # Tell the server we accept application/mercurial-0.2 and multiple
        # compression formats if the server is capable of emitting those
        # payloads.
        protoparams = []

        mediatypes = set()
        if self.caps is not None:
            mt = self.capable('httpmediatype')
            if mt:
                protoparams.append('0.1')
                mediatypes = set(mt.split(','))

        if '0.2tx' in mediatypes:
            protoparams.append('0.2')

        if '0.2tx' in mediatypes and self.capable('compression'):
            # We /could/ compare supported compression formats and prune
            # non-mutually supported or error if nothing is mutually supported.
            # For now, send the full list to the server and have it error.
            comps = [e.wireprotosupport().name for e in
                     util.compengines.supportedwireengines(util.CLIENTROLE)]
            protoparams.append('comp=%s' % ','.join(comps))

        if protoparams:
            protoheaders = encodevalueinheaders(' '.join(protoparams),
                                                'X-HgProto',
                                                headersize or 1024)
            for header, value in protoheaders:
                headers[header] = value
                varyheaders.append(header)

        if varyheaders:
            headers['Vary'] = ','.join(varyheaders)

        req = self.requestbuilder(cu, data, headers)

        if data is not None:
            self.ui.debug("sending %s bytes\n" % size)
            req.add_unredirected_header('Content-Length', '%d' % size)
        try:
            resp = self.urlopener.open(req)
        except urlerr.httperror as inst:
            if inst.code == 401:
                raise error.Abort(_('authorization failed'))
            raise
        except httplib.HTTPException as inst:
            self.ui.debug('http error while sending %s command\n' % cmd)
            self.ui.traceback()
            raise IOError(None, inst)

        # Insert error handlers for common I/O failures.
        _wraphttpresponse(resp)

        # record the url we got redirected to
        resp_url = resp.geturl()
        if resp_url.endswith(qs):
            resp_url = resp_url[:-len(qs)]
        if self._url.rstrip('/') != resp_url.rstrip('/'):
            if not self.ui.quiet:
                self.ui.warn(_('real URL is %s\n') % resp_url)
        self._url = resp_url
        try:
            proto = resp.getheader('content-type')
        except AttributeError:
            proto = resp.headers.get('content-type', '')

        safeurl = util.hidepassword(self._url)
        if proto.startswith('application/hg-error'):
            raise error.OutOfBandError(resp.read())
        # accept old "text/plain" and "application/hg-changegroup" for now
        if not (proto.startswith('application/mercurial-') or
                (proto.startswith('text/plain')
                 and not resp.headers.get('content-length')) or
                proto.startswith('application/hg-changegroup')):
            self.ui.debug("requested URL: '%s'\n" % util.hidepassword(cu))
            raise error.RepoError(
                _("'%s' does not appear to be an hg repository:\n"
                  "---%%<--- (%s)\n%s\n---%%<---\n")
                % (safeurl, proto or 'no content-type', resp.read(1024)))

        if proto.startswith('application/mercurial-'):
            try:
                version = proto.split('-', 1)[1]
                version_info = tuple([int(n) for n in version.split('.')])
            except ValueError:
                raise error.RepoError(_("'%s' sent a broken Content-Type "
                                        "header (%s)") % (safeurl, proto))

            # TODO consider switching to a decompression reader that uses
            # generators.
            if version_info == (0, 1):
                if _compressible:
                    return util.compengines['zlib'].decompressorreader(resp)
                return resp
            elif version_info == (0, 2):
                # application/mercurial-0.2 always identifies the compression
                # engine in the payload header.
                elen = struct.unpack('B', resp.read(1))[0]
                ename = resp.read(elen)
                engine = util.compengines.forwiretype(ename)
                return engine.decompressorreader(resp)
            else:
                raise error.RepoError(_("'%s' uses newer protocol %s") %
                                      (safeurl, version))

        if _compressible:
            return util.compengines['zlib'].decompressorreader(resp)

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

        types = self.capable('unbundle')
        try:
            types = types.split(',')
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
        headers = {'Content-Type': 'application/mercurial-0.1'}

        try:
            r = self._call(cmd, data=fp, headers=headers, **args)
            vals = r.split('\n', 1)
            if len(vals) < 2:
                raise error.ResponseError(_("unexpected response:"), r)
            return vals
        except socket.error as err:
            if err.args[0] in (errno.ECONNRESET, errno.EPIPE):
                raise error.Abort(_('push failed: %s') % err.args[1])
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
            fh = os.fdopen(fd, pycompat.sysstr("wb"))
            d = fp.read(4096)
            while d:
                fh.write(d)
                d = fp.read(4096)
            fh.close()
            # start http push
            fp_ = httpconnection.httpsendfile(self.ui, filename, "rb")
            headers = {'Content-Type': 'application/mercurial-0.1'}
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
            raise error.Abort(_('Python support for SSL and HTTPS '
                               'is not installed'))
        httppeer.__init__(self, ui, path)

def instance(ui, path, create):
    if create:
        raise error.Abort(_('cannot create new http repository'))
    try:
        if path.startswith('https:'):
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
    except error.RepoError as httpexception:
        try:
            r = statichttprepo.instance(ui, "static-" + path, create)
            ui.note(_('(falling back to static-http)\n'))
            return r
        except error.RepoError:
            raise httpexception # use the original http RepoError instead
