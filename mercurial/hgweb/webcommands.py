#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import cStringIO, zlib, tempfile, errno, os, sys
from mercurial import revlog, util, streamclone
from mercurial.i18n import gettext as _
from mercurial.node import *
from common import staticfile

def log(web, req):
    if req.form.has_key('file') and req.form['file'][0]:
        filelog(web, req)
    else:
        changelog(web, req)

def file(web, req):
    path = web.cleanpath(req.form.get('file', [''])[0])
    if path:
        try:
            req.write(web.filerevision(web.filectx(req)))
            return
        except revlog.LookupError:
            pass

    req.write(web.manifest(web.changectx(req), path))

def changelog(web, req, shortlog = False):
    if req.form.has_key('node'):
        ctx = web.changectx(req)
    else:
        if req.form.has_key('rev'):
            hi = req.form['rev'][0]
        else:
            hi = web.repo.changelog.count() - 1
        try:
            ctx = web.repo.changectx(hi)
        except hg.RepoError:
            req.write(web.search(hi)) # XXX redirect to 404 page?
            return

    req.write(web.changelog(ctx, shortlog = shortlog))

def shortlog(web, req):
    changelog(web, req, shortlog = True)

def changeset(web, req):
    req.write(web.changeset(web.changectx(req)))

rev = changeset

def manifest(web, req):
    req.write(web.manifest(web.changectx(req),
                           web.cleanpath(req.form['path'][0])))

def tags(web, req):
    req.write(web.tags())

def summary(web, req):
    req.write(web.summary())

def filediff(web, req):
    req.write(web.filediff(web.filectx(req)))

diff = filediff

def annotate(web, req):
    req.write(web.fileannotate(web.filectx(req)))

def filelog(web, req):
    req.write(web.filelog(web.filectx(req)))

def lookup(web, req):
    try:
        r = hex(web.repo.lookup(req.form['key'][0]))
        success = 1
    except Exception,inst:
        r = str(inst)
        success = 0
    resp = "%s %s\n" % (success, r)
    req.httphdr("application/mercurial-0.1", length=len(resp))
    req.write(resp)

def heads(web, req):
    resp = " ".join(map(hex, web.repo.heads())) + "\n"
    req.httphdr("application/mercurial-0.1", length=len(resp))
    req.write(resp)

def branches(web, req):
    nodes = []
    if req.form.has_key('nodes'):
        nodes = map(bin, req.form['nodes'][0].split(" "))
    resp = cStringIO.StringIO()
    for b in web.repo.branches(nodes):
        resp.write(" ".join(map(hex, b)) + "\n")
    resp = resp.getvalue()
    req.httphdr("application/mercurial-0.1", length=len(resp))
    req.write(resp)

def between(web, req):
    if req.form.has_key('pairs'):
        pairs = [map(bin, p.split("-"))
                 for p in req.form['pairs'][0].split(" ")]
    resp = cStringIO.StringIO()
    for b in web.repo.between(pairs):
        resp.write(" ".join(map(hex, b)) + "\n")
    resp = resp.getvalue()
    req.httphdr("application/mercurial-0.1", length=len(resp))
    req.write(resp)

def changegroup(web, req):
    req.httphdr("application/mercurial-0.1")
    nodes = []
    if not web.allowpull:
        return

    if req.form.has_key('roots'):
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
    req.httphdr("application/mercurial-0.1")
    bases = []
    heads = []
    if not web.allowpull:
        return

    if req.form.has_key('bases'):
        bases = [bin(x) for x in req.form['bases'][0].split(' ')]
    if req.form.has_key('heads'):
        heads = [bin(x) for x in req.form['heads'][0].split(' ')]

    z = zlib.compressobj()
    f = web.repo.changegroupsubset(bases, heads, 'serve')
    while 1:
        chunk = f.read(4096)
        if not chunk:
            break
        req.write(z.compress(chunk))

    req.write(z.flush())

def archive(web, req):
    type_ = req.form['type'][0]
    allowed = web.configlist("web", "allow_archive")
    if (type_ in web.archives and (type_ in allowed or
        web.configbool("web", "allow" + type_, False))):
        web.archive(req, req.form['node'][0], type_)
        return

    req.respond(400, web.t('error',
                           error='Unsupported archive type: %s' % type_))

def static(web, req):
    fname = req.form['file'][0]
    # a repo owner may set web.static in .hg/hgrc to get any file
    # readable by the user running the CGI script
    static = web.config("web", "static",
                        os.path.join(web.templatepath, "static"),
                        untrusted=False)
    req.write(staticfile(static, fname, req))

def capabilities(web, req):
    caps = ['lookup', 'changegroupsubset']
    if web.configbool('server', 'uncompressed'):
        caps.append('stream=%d' % web.repo.changelog.version)
    # XXX: make configurable and/or share code with do_unbundle:
    unbundleversions = ['HG10GZ', 'HG10BZ', 'HG10UN']
    if unbundleversions:
        caps.append('unbundle=%s' % ','.join(unbundleversions))
    resp = ' '.join(caps)
    req.httphdr("application/mercurial-0.1", length=len(resp))
    req.write(resp)

def unbundle(web, req):
    def bail(response, headers={}):
        length = int(req.env['CONTENT_LENGTH'])
        for s in util.filechunkiter(req, limit=length):
            # drain incoming bundle, else client will not see
            # response when run outside cgi script
            pass
        req.httphdr("application/mercurial-0.1", headers=headers)
        req.write('0\n')
        req.write(response)

    # require ssl by default, auth info cannot be sniffed and
    # replayed
    ssl_req = web.configbool('web', 'push_ssl', True)
    if ssl_req:
        if req.env.get('wsgi.url_scheme') != 'https':
            bail(_('ssl required\n'))
            return
        proto = 'https'
    else:
        proto = 'http'

    # do not allow push unless explicitly allowed
    if not web.check_perm(req, 'push', False):
        bail(_('push not authorized\n'),
             headers={'status': '401 Unauthorized'})
        return

    their_heads = req.form['heads'][0].split(' ')

    def check_heads():
        heads = map(hex, web.repo.heads())
        return their_heads == [hex('force')] or their_heads == heads

    # fail early if possible
    if not check_heads():
        bail(_('unsynced changes\n'))
        return

    req.httphdr("application/mercurial-0.1")

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
                    req.write(_('unsynced changes\n'))
                    return

                fp.seek(0)
                header = fp.read(6)
                if not header.startswith("HG"):
                    # old client with uncompressed bundle
                    def generator(f):
                        yield header
                        for chunk in f:
                            yield chunk
                elif not header.startswith("HG10"):
                    req.write("0\n")
                    req.write(_("unknown bundle version\n"))
                    return
                elif header == "HG10GZ":
                    def generator(f):
                        zd = zlib.decompressobj()
                        for chunk in f:
                            yield zd.decompress(chunk)
                elif header == "HG10BZ":
                    def generator(f):
                        zd = bz2.BZ2Decompressor()
                        zd.decompress("BZ")
                        for chunk in f:
                            yield zd.decompress(chunk)
                elif header == "HG10UN":
                    def generator(f):
                        for chunk in f:
                            yield chunk
                else:
                    req.write("0\n")
                    req.write(_("unknown bundle compression type\n"))
                    return
                gen = generator(util.filechunkiter(fp, 4096))

                # send addchangegroup output to client

                old_stdout = sys.stdout
                sys.stdout = cStringIO.StringIO()

                try:
                    url = 'remote:%s:%s' % (proto,
                                            req.env.get('REMOTE_HOST', ''))
                    try:
                        ret = web.repo.addchangegroup(
                                    util.chunkbuffer(gen), 'serve', url)
                    except util.Abort, inst:
                        sys.stdout.write("abort: %s\n" % inst)
                        ret = 0
                finally:
                    val = sys.stdout.getvalue()
                    sys.stdout = old_stdout
                req.write('%d\n' % ret)
                req.write(val)
            finally:
                del lock
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
                code = 404
            else:
                code = 500
            req.respond(code, '%s: %s\n' % (error, filename))
    finally:
        fp.close()
        os.unlink(tempname)

def stream_out(web, req):
    req.httphdr("application/mercurial-0.1")
    streamclone.stream_out(web.repo, req, untrusted=True)
