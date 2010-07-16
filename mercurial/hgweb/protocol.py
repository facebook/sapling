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
    def sendresponse(self, s):
        self.req.respond(HTTP_OK, HGTYPE, length=len(s))
        self.response = s
    def sendchangegroup(self, cg):
        self.req.respond(HTTP_OK, HGTYPE)
        z = zlib.compressobj()
        while 1:
            chunk = cg.read(4096)
            if not chunk:
                break
            self.req.write(z.compress(chunk))
        self.req.write(z.flush())
    def sendstream(self, source):
        self.req.respond(HTTP_OK, HGTYPE)
        for chunk in source:
            self.req.write(chunk)
    def sendpushresponse(self, ret):
        val = sys.stdout.getvalue()
        sys.stdout, sys.stderr = self.oldio
        self.req.respond(HTTP_OK, HGTYPE)
        self.response = '%d\n%s' % (ret, val)
    def _client(self):
        return 'remote:%s:%s:%s' % (
            self.req.env.get('wsgi.url_scheme') or 'http',
            urllib.quote(self.req.env.get('REMOTE_HOST', '')),
            urllib.quote(self.req.env.get('REMOTE_USER', '')))

def iscmd(cmd):
    return cmd in wireproto.commands

def call(repo, req, cmd):
    p = webproto(req)
    wireproto.dispatch(repo, p, cmd)
    yield p.response
