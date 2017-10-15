#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import cgi
import struct

from .common import (
    HTTP_OK,
)

from .. import (
    error,
    pycompat,
    util,
    wireproto,
)
stringio = util.stringio

urlerr = util.urlerr
urlreq = util.urlreq

HGTYPE = 'application/mercurial-0.1'
HGTYPE2 = 'application/mercurial-0.2'
HGERRTYPE = 'application/hg-error'

def decodevaluefromheaders(req, headerprefix):
    """Decode a long value from multiple HTTP request headers.

    Returns the value as a bytes, not a str.
    """
    chunks = []
    i = 1
    prefix = headerprefix.upper().replace(r'-', r'_')
    while True:
        v = req.env.get(r'HTTP_%s_%d' % (prefix, i))
        if v is None:
            break
        chunks.append(pycompat.bytesurl(v))
        i += 1

    return ''.join(chunks)

class webproto(wireproto.abstractserverproto):
    def __init__(self, req, ui):
        self.req = req
        self.response = ''
        self.ui = ui
        self.name = 'http'

    def getargs(self, args):
        knownargs = self._args()
        data = {}
        keys = args.split()
        for k in keys:
            if k == '*':
                star = {}
                for key in knownargs.keys():
                    if key != 'cmd' and key not in keys:
                        star[key] = knownargs[key][0]
                data['*'] = star
            else:
                data[k] = knownargs[k][0]
        return [data[k] for k in keys]
    def _args(self):
        args = self.req.form.copy()
        if pycompat.ispy3:
            args = {k.encode('ascii'): [v.encode('ascii') for v in vs]
                    for k, vs in args.items()}
        postlen = int(self.req.env.get(r'HTTP_X_HGARGS_POST', 0))
        if postlen:
            args.update(cgi.parse_qs(
                self.req.read(postlen), keep_blank_values=True))
            return args

        argvalue = decodevaluefromheaders(self.req, r'X-HgArg')
        args.update(cgi.parse_qs(argvalue, keep_blank_values=True))
        return args
    def getfile(self, fp):
        length = int(self.req.env[r'CONTENT_LENGTH'])
        # If httppostargs is used, we need to read Content-Length
        # minus the amount that was consumed by args.
        length -= int(self.req.env.get(r'HTTP_X_HGARGS_POST', 0))
        for s in util.filechunkiter(self.req, limit=length):
            fp.write(s)
    def redirect(self):
        self.oldio = self.ui.fout, self.ui.ferr
        self.ui.ferr = self.ui.fout = stringio()
    def restore(self):
        val = self.ui.fout.getvalue()
        self.ui.ferr, self.ui.fout = self.oldio
        return val

    def _client(self):
        return 'remote:%s:%s:%s' % (
            self.req.env.get('wsgi.url_scheme') or 'http',
            urlreq.quote(self.req.env.get('REMOTE_HOST', '')),
            urlreq.quote(self.req.env.get('REMOTE_USER', '')))

    def responsetype(self, v1compressible=False):
        """Determine the appropriate response type and compression settings.

        The ``v1compressible`` argument states whether the response with
        application/mercurial-0.1 media types should be zlib compressed.

        Returns a tuple of (mediatype, compengine, engineopts).
        """
        # For now, if it isn't compressible in the old world, it's never
        # compressible. We can change this to send uncompressed 0.2 payloads
        # later.
        if not v1compressible:
            return HGTYPE, None, None

        # Determine the response media type and compression engine based
        # on the request parameters.
        protocaps = decodevaluefromheaders(self.req, r'X-HgProto').split(' ')

        if '0.2' in protocaps:
            # Default as defined by wire protocol spec.
            compformats = ['zlib', 'none']
            for cap in protocaps:
                if cap.startswith('comp='):
                    compformats = cap[5:].split(',')
                    break

            # Now find an agreed upon compression format.
            for engine in wireproto.supportedcompengines(self.ui, self,
                                                         util.SERVERROLE):
                if engine.wireprotosupport().name in compformats:
                    opts = {}
                    level = self.ui.configint('server',
                                              '%slevel' % engine.name())
                    if level is not None:
                        opts['level'] = level

                    return HGTYPE2, engine, opts

            # No mutually supported compression format. Fall back to the
            # legacy protocol.

        # Don't allow untrusted settings because disabling compression or
        # setting a very high compression level could lead to flooding
        # the server's network or CPU.
        opts = {'level': self.ui.configint('server', 'zliblevel')}
        return HGTYPE, util.compengines['zlib'], opts

def iscmd(cmd):
    return cmd in wireproto.commands

def call(repo, req, cmd):
    p = webproto(req, repo.ui)

    def genversion2(gen, compress, engine, engineopts):
        # application/mercurial-0.2 always sends a payload header
        # identifying the compression engine.
        name = engine.wireprotosupport().name
        assert 0 < len(name) < 256
        yield struct.pack('B', len(name))
        yield name

        if compress:
            for chunk in engine.compressstream(gen, opts=engineopts):
                yield chunk
        else:
            for chunk in gen:
                yield chunk

    rsp = wireproto.dispatch(repo, p, cmd)
    if isinstance(rsp, bytes):
        req.respond(HTTP_OK, HGTYPE, body=rsp)
        return []
    elif isinstance(rsp, wireproto.streamres):
        if rsp.reader:
            gen = iter(lambda: rsp.reader.read(32768), '')
        else:
            gen = rsp.gen

        # This code for compression should not be streamres specific. It
        # is here because we only compress streamres at the moment.
        mediatype, engine, engineopts = p.responsetype(rsp.v1compressible)

        if mediatype == HGTYPE and rsp.v1compressible:
            gen = engine.compressstream(gen, engineopts)
        elif mediatype == HGTYPE2:
            gen = genversion2(gen, rsp.v1compressible, engine, engineopts)

        req.respond(HTTP_OK, mediatype)
        return gen
    elif isinstance(rsp, wireproto.pushres):
        val = p.restore()
        rsp = '%d\n%s' % (rsp.res, val)
        req.respond(HTTP_OK, HGTYPE, body=rsp)
        return []
    elif isinstance(rsp, wireproto.pusherr):
        # drain the incoming bundle
        req.drain()
        p.restore()
        rsp = '0\n%s\n' % rsp.res
        req.respond(HTTP_OK, HGTYPE, body=rsp)
        return []
    elif isinstance(rsp, wireproto.ooberror):
        rsp = rsp.message
        req.respond(HTTP_OK, HGERRTYPE, body=rsp)
        return []
    raise error.ProgrammingError('hgweb.protocol internal failure', rsp)
