#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import cStringIO, zlib, sys, urllib
from mercurial import util, wireproto
from common import HTTP_OK

HGTYPE = 'application/mercurial-0.1'

class webproto(object):
    def __init__(self, req):
        self.req = req
        self.response = ''
    def getargs(self, args):
        data = {}
        keys = args.split()
        for k in keys:
            if k == '*':
                star = {}
                for key in self.req.form.keys():
                    if key not in keys:
                        star[key] = self.req.form[key][0]
                data['*'] = star
            else:
                data[k] = self.req.form[k][0]
        return [data[k] for k in keys]
    def getfile(self, fp):
        length = int(self.req.env['CONTENT_LENGTH'])
        for s in util.filechunkiter(self.req, limit=length):
            fp.write(s)
    def redirect(self):
        self.oldio = sys.stdout, sys.stderr
        sys.stderr = sys.stdout = cStringIO.StringIO()
    def groupchunks(self, cg):
        z = zlib.compressobj()
        while 1:
            chunk = cg.read(4096)
            if not chunk:
                break
            yield z.compress(chunk)
        yield z.flush()
    def _client(self):
        return 'remote:%s:%s:%s' % (
            self.req.env.get('wsgi.url_scheme') or 'http',
            urllib.quote(self.req.env.get('REMOTE_HOST', '')),
            urllib.quote(self.req.env.get('REMOTE_USER', '')))

def iscmd(cmd):
    return cmd in wireproto.commands

def call(repo, req, cmd):
    p = webproto(req)
    rsp = wireproto.dispatch(repo, p, cmd)
    if isinstance(rsp, str):
        req.respond(HTTP_OK, HGTYPE, length=len(rsp))
        return [rsp]
    elif isinstance(rsp, wireproto.streamres):
        req.respond(HTTP_OK, HGTYPE)
        return rsp.gen
    elif isinstance(rsp, wireproto.pushres):
        val = sys.stdout.getvalue()
        sys.stdout, sys.stderr = p.oldio
        req.respond(HTTP_OK, HGTYPE)
        return ['%d\n%s' % (rsp.res, val)]
    elif isinstance(rsp, wireproto.pusherr):
        # drain the incoming bundle
        req.drain()
        sys.stdout, sys.stderr = p.oldio
        rsp = '0\n%s\n' % rsp.res
        req.respond(HTTP_OK, HGTYPE, length=len(rsp))
        return [rsp]
