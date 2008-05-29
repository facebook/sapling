#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import cStringIO, zlib, tempfile, errno, os, sys
from mercurial import util, streamclone
from mercurial.node import bin, hex
from mercurial import changegroup as changegroupmod
from common import HTTP_OK, HTTP_NOT_FOUND, HTTP_SERVER_ERROR

# __all__ is populated with the allowed commands. Be sure to add to it if
# you're adding a new command, or the new command won't work.

__all__ = [
   'lookup', 'heads', 'branches', 'between', 'changegroup',
   'changegroupsubset', 'capabilities', 'unbundle', 'stream_out',
]

HGTYPE = 'application/mercurial-0.1'

def lookup(web, req):
    try:
        r = hex(web.repo.lookup(req.form['key'][0]))
        success = 1
    except Exception,inst:
        r = str(inst)
        success = 0
    resp = "%s %s\n" % (success, r)
    req.respond(HTTP_OK, HGTYPE, length=len(resp))
    req.write(resp)

def heads(web, req):
    resp = " ".join(map(hex, web.repo.heads())) + "\n"
    req.respond(HTTP_OK, HGTYPE, length=len(resp))
    req.write(resp)

def branches(web, req):
    nodes = []
    if 'nodes' in req.form:
        nodes = map(bin, req.form['nodes'][0].split(" "))
    resp = cStringIO.StringIO()
    for b in web.repo.branches(nodes):
        resp.write(" ".join(map(hex, b)) + "\n")
    resp = resp.getvalue()
    req.respond(HTTP_OK, HGTYPE, length=len(resp))
    req.write(resp)

def between(web, req):
    if 'pairs' in req.form:
        pairs = [map(bin, p.split("-"))
                 for p in req.form['pairs'][0].split(" ")]
    resp = cStringIO.StringIO()
    for b in web.repo.between(pairs):
        resp.write(" ".join(map(hex, b)) + "\n")
    resp = resp.getvalue()
    req.respond(HTTP_OK, HGTYPE, length=len(resp))
    req.write(resp)

def changegroup(web, req):
    req.respond(HTTP_OK, HGTYPE)
    nodes = []
    if not web.allowpull:
        return

    if 'roots' in req.form:
        nodes = map(bin, req.form['roots'][0].split(" "))

    z = zlib.compressobj()
    f = web.repo.changegroup(nodes, 'serve')
    while 1:
        chunk = f.read(4096)
        if not chunk:
            break
        req.write(z.compress(chunk))

    req.write(z.flush())

def changegroupsubset(web, req):
    req.respond(HTTP_OK, HGTYPE)
    bases = []
    heads = []
    if not web.allowpull:
        return

    if 'bases' in req.form:
        bases = [bin(x) for x in req.form['bases'][0].split(' ')]
    if 'heads' in req.form:
        heads = [bin(x) for x in req.form['heads'][0].split(' ')]

    z = zlib.compressobj()
    f = web.repo.changegroupsubset(bases, heads, 'serve')
    while 1:
        chunk = f.read(4096)
        if not chunk:
            break
        req.write(z.compress(chunk))

    req.write(z.flush())

def capabilities(web, req):
    resp = ' '.join(web.capabilities())
    req.respond(HTTP_OK, HGTYPE, length=len(resp))
    req.write(resp)

def unbundle(web, req):

    def bail(response, headers={}):
        length = int(req.env.get('CONTENT_LENGTH', 0))
        for s in util.filechunkiter(req, limit=length):
            # drain incoming bundle, else client will not see
            # response when run outside cgi script
            pass

        status = headers.pop('status', HTTP_OK)
        req.header(headers.items())
        req.respond(status, HGTYPE)
        req.write('0\n')
        req.write(response)

    # enforce that you can only unbundle with POST requests
    if req.env['REQUEST_METHOD'] != 'POST':
        headers = {'status': '405 Method Not Allowed'}
        bail('unbundle requires POST request\n', headers)
        return

    # require ssl by default, auth info cannot be sniffed and
    # replayed
    ssl_req = web.configbool('web', 'push_ssl', True)
    if ssl_req:
        if req.env.get('wsgi.url_scheme') != 'https':
            bail('ssl required\n')
            return
        proto = 'https'
    else:
        proto = 'http'

    # do not allow push unless explicitly allowed
    if not web.check_perm(req, 'push', False):
        bail('push not authorized\n', headers={'status': '401 Unauthorized'})
        return

    their_heads = req.form['heads'][0].split(' ')

    def check_heads():
        heads = map(hex, web.repo.heads())
        return their_heads == [hex('force')] or their_heads == heads

    # fail early if possible
    if not check_heads():
        bail('unsynced changes\n')
        return

    req.respond(HTTP_OK, HGTYPE)

    # do not lock repo until all changegroup data is
    # streamed. save to temporary file.

    fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
    fp = os.fdopen(fd, 'wb+')
    try:
        length = int(req.env['CONTENT_LENGTH'])
        for s in util.filechunkiter(req, limit=length):
            fp.write(s)

        try:
            lock = web.repo.lock()
            try:
                if not check_heads():
                    req.write('0\n')
                    req.write('unsynced changes\n')
                    return

                fp.seek(0)
                header = fp.read(6)
                if header.startswith('HG') and not header.startswith('HG10'):
                    raise ValueError('unknown bundle version')
                elif header not in changegroupmod.bundletypes:
                    raise ValueError('unknown bundle compression type')
                gen = changegroupmod.unbundle(header, fp)

                # send addchangegroup output to client

                oldio = sys.stdout, sys.stderr
                sys.stderr = sys.stdout = cStringIO.StringIO()

                try:
                    url = 'remote:%s:%s' % (proto,
                                            req.env.get('REMOTE_HOST', ''))
                    try:
                        ret = web.repo.addchangegroup(gen, 'serve', url)
                    except util.Abort, inst:
                        sys.stdout.write("abort: %s\n" % inst)
                        ret = 0
                finally:
                    val = sys.stdout.getvalue()
                    sys.stdout, sys.stderr = oldio
                req.write('%d\n' % ret)
                req.write(val)
            finally:
                del lock
        except ValueError, inst:
            req.write('0\n')
            req.write(str(inst) + '\n')
        except (OSError, IOError), inst:
            req.write('0\n')
            filename = getattr(inst, 'filename', '')
            # Don't send our filesystem layout to the client
            if filename.startswith(web.repo.root):
                filename = filename[len(web.repo.root)+1:]
            else:
                filename = ''
            error = getattr(inst, 'strerror', 'Unknown error')
            if inst.errno == errno.ENOENT:
                code = HTTP_NOT_FOUND
            else:
                code = HTTP_SERVER_ERROR
            req.respond(code)
            req.write('%s: %s\n' % (error, filename))
    finally:
        fp.close()
        os.unlink(tempname)

def stream_out(web, req):
    if not web.allowpull:
        return
    req.respond(HTTP_OK, HGTYPE)
    streamclone.stream_out(web.repo, req, untrusted=True)
