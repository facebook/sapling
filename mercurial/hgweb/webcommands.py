#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
from mercurial import revlog
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
