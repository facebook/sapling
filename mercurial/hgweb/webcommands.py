#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, mimetypes
from mercurial import revlog, util
from common import staticfile, ErrorResponse

def log(web, req, tmpl):
    if 'file' in req.form and req.form['file'][0]:
        filelog(web, req, tmpl)
    else:
        changelog(web, req, tmpl)

def rawfile(web, req, tmpl):
    path = web.cleanpath(req.form.get('file', [''])[0])
    if not path:
        req.write(web.manifest(tmpl, web.changectx(req), path))
        return

    try:
        fctx = web.filectx(req)
    except revlog.LookupError:
        req.write(web.manifest(tmpl, web.changectx(req), path))
        return

    path = fctx.path()
    text = fctx.data()
    mt = mimetypes.guess_type(path)[0]
    if mt is None or util.binary(text):
        mt = mt or 'application/octet-stream'

    req.httphdr(mt, path, len(text))
    req.write(text)

def file(web, req, tmpl):
    path = web.cleanpath(req.form.get('file', [''])[0])
    if path:
        try:
            req.write(web.filerevision(tmpl, web.filectx(req)))
            return
        except revlog.LookupError:
            pass

    req.write(web.manifest(tmpl, web.changectx(req), path))

def changelog(web, req, tmpl, shortlog = False):
    if 'node' in req.form:
        ctx = web.changectx(req)
    else:
        if 'rev' in req.form:
            hi = req.form['rev'][0]
        else:
            hi = web.repo.changelog.count() - 1
        try:
            ctx = web.repo.changectx(hi)
        except hg.RepoError:
            req.write(web.search(tmpl, hi)) # XXX redirect to 404 page?
            return

    req.write(web.changelog(tmpl, ctx, shortlog = shortlog))

def shortlog(web, req, tmpl):
    changelog(web, req, tmpl, shortlog = True)

def changeset(web, req, tmpl):
    req.write(web.changeset(tmpl, web.changectx(req)))

rev = changeset

def manifest(web, req, tmpl):
    req.write(web.manifest(tmpl, web.changectx(req),
                           web.cleanpath(req.form['path'][0])))

def tags(web, req, tmpl):
    req.write(web.tags(tmpl))

def summary(web, req, tmpl):
    req.write(web.summary(tmpl))

def filediff(web, req, tmpl):
    req.write(web.filediff(tmpl, web.filectx(req)))

diff = filediff

def annotate(web, req, tmpl):
    req.write(web.fileannotate(tmpl, web.filectx(req)))

def filelog(web, req, tmpl):
    req.write(web.filelog(tmpl, web.filectx(req)))

def archive(web, req, tmpl):
    type_ = req.form['type'][0]
    allowed = web.configlist("web", "allow_archive")
    if (type_ in web.archives and (type_ in allowed or
        web.configbool("web", "allow" + type_, False))):
        web.archive(tmpl, req, req.form['node'][0], type_)
        return

    raise ErrorResponse(400, 'Unsupported archive type: %s' % type_)

def static(web, req, tmpl):
    fname = req.form['file'][0]
    # a repo owner may set web.static in .hg/hgrc to get any file
    # readable by the user running the CGI script
    static = web.config("web", "static",
                        os.path.join(web.templatepath, "static"),
                        untrusted=False)
    req.write(staticfile(static, fname, req))
