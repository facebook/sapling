# httprepo.py - HTTP repository proxy classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid
from i18n import _
import changegroup, statichttprepo, error, url, util, wireproto
import os, urllib, urllib2, urlparse, zlib, httplib
import errno, socket

def zgenerator(f):
    zd = zlib.decompressobj()
    try:
        for chunk in util.filechunkiter(f):
            while chunk:
                yield zd.decompress(chunk, 2**18)
                chunk = zd.unconsumed_tail
    except httplib.HTTPException:
        raise IOError(None, _('connection ended unexpectedly'))
    yield zd.flush()

class httprepository(wireproto.wirerepository):
    def __init__(self, ui, path):
        self.path = path
        self.caps = None
        self.handler = None
        scheme, netloc, urlpath, query, frag = urlparse.urlsplit(path)
        if query or frag:
            raise util.Abort(_('unsupported URL component: "%s"') %
                             (query or frag))

        # urllib cannot handle URLs with embedded user or passwd
        self._url, authinfo = url.getauthinfo(path)

        self.ui = ui
        self.ui.debug('using %s\n' % self._url)

        self.urlopener = url.opener(ui, authinfo)

    def __del__(self):
        for h in self.urlopener.handlers:
            h.close()
            if hasattr(h, "close_all"):
                h.close_all()

    def url(self):
        return self.path

    # look up capabilities only when needed

    def get_caps(self):
        if self.caps is None:
            try:
                self.caps = set(self._call('capabilities').split())
            except error.RepoError:
                self.caps = set()
            self.ui.debug('capabilities: %s\n' %
                          (' '.join(self.caps or ['none'])))
        return self.caps

    capabilities = property(get_caps)

    def lock(self):
        raise util.Abort(_('operation not supported over http'))

    def _callstream(self, cmd, **args):
        if cmd == 'pushkey':
            args['data'] = ''
        data = args.pop('data', None)
        headers = args.pop('headers', {})
        self.ui.debug("sending %s command\n" % cmd)
        q = {"cmd": cmd}
        q.update(args)
        qs = '?%s' % urllib.urlencode(q)
        cu = "%s%s" % (self._url, qs)
        req = urllib2.Request(cu, data, headers)
        if data is not None:
            # len(data) is broken if data doesn't fit into Py_ssize_t
            # add the header ourself to avoid OverflowError
            size = data.__len__()
            self.ui.debug("sending %s bytes\n" % size)
            req.add_unredirected_header('Content-Length', '%d' % size)
        try:
            resp = self.urlopener.open(req)
        except urllib2.HTTPError, inst:
            if inst.code == 401:
                raise util.Abort(_('authorization failed'))
            raise
        except httplib.HTTPException, inst:
            self.ui.debug('http error while sending %s command\n' % cmd)
            self.ui.traceback()
            raise IOError(None, inst)
        except IndexError:
            # this only happens with Python 2.3, later versions raise URLError
            raise util.Abort(_('http error, possibly caused by proxy setting'))
        # record the url we got redirected to
        resp_url = resp.geturl()
        if resp_url.endswith(qs):
            resp_url = resp_url[:-len(qs)]
        if self._url.rstrip('/') != resp_url.rstrip('/'):
            self.ui.status(_('real URL is %s\n') % resp_url)
        self._url = resp_url
        try:
            proto = resp.getheader('content-type')
        except AttributeError:
            proto = resp.headers['content-type']

        safeurl = url.hidepassword(self._url)
        # accept old "text/plain" and "application/hg-changegroup" for now
        if not (proto.startswith('application/mercurial-') or
                proto.startswith('text/plain') or
                proto.startswith('application/hg-changegroup')):
            self.ui.debug("requested URL: '%s'\n" % url.hidepassword(cu))
            raise error.RepoError(
                _("'%s' does not appear to be an hg repository:\n"
                  "---%%<--- (%s)\n%s\n---%%<---\n")
                % (safeurl, proto, resp.read()))

        if proto.startswith('application/mercurial-'):
            try:
                version = proto.split('-', 1)[1]
                version_info = tuple([int(n) for n in version.split('.')])
            except ValueError:
                raise error.RepoError(_("'%s' sent a broken Content-Type "
                                        "header (%s)") % (safeurl, proto))
            if version_info > (0, 1):
                raise error.RepoError(_("'%s' uses newer protocol %s") %
                                      (safeurl, version))

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

        type = ""
        types = self.capable('unbundle')
        # servers older than d1b16a746db6 will send 'unbundle' as a
        # boolean capability
        try:
            types = types.split(',')
        except AttributeError:
            types = [""]
        if types:
            for x in types:
                if x in changegroup.bundletypes:
                    type = x
                    break

        tempname = changegroup.writebundle(cg, None, type)
        fp = url.httpsendfile(self.ui, tempname, "rb")
        headers = {'Content-Type': 'application/mercurial-0.1'}

        try:
            try:
                r = self._call(cmd, data=fp, headers=headers, **args)
                return r.split('\n', 1)
            except socket.error, err:
                if err.args[0] in (errno.ECONNRESET, errno.EPIPE):
                    raise util.Abort(_('push failed: %s') % err.args[1])
                raise util.Abort(err.args[1])
        finally:
            fp.close()
            os.unlink(tempname)

    def _abort(self, exception):
        raise exception

    def _decompress(self, stream):
        return util.chunkbuffer(zgenerator(stream))

class httpsrepository(httprepository):
    def __init__(self, ui, path):
        if not url.has_https:
            raise util.Abort(_('Python support for SSL and HTTPS '
                               'is not installed'))
        httprepository.__init__(self, ui, path)

def instance(ui, path, create):
    if create:
        raise util.Abort(_('cannot create new http repository'))
    try:
        if path.startswith('https:'):
            inst = httpsrepository(ui, path)
        else:
            inst = httprepository(ui, path)
        inst.between([(nullid, nullid)])
        return inst
    except error.RepoError:
        ui.note('(falling back to static-http)\n')
        return statichttprepo.instance(ui, "static-" + path, create)
