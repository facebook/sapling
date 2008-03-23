#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, mimetypes
from mercurial import revlog, util
from mercurial.repo import RepoError
from common import staticfile, ErrorResponse, HTTP_OK, HTTP_NOT_FOUND

# __all__ is populated with the allowed commands. Be sure to add to it if
# you're adding a new command, or the new command won't work.

__all__ = [
   'log', 'rawfile', 'file', 'changelog', 'shortlog', 'changeset', 'rev',
   'manifest', 'tags', 'summary', 'filediff', 'diff', 'annotate', 'filelog',
   'archive', 'static',
]

def log(web, req, tmpl):
    if 'file' in req.form and req.form['file'][0]:
        return filelog(web, req, tmpl)
    else:
        return changelog(web, req, tmpl)

def rawfile(web, req, tmpl):
    path = web.cleanpath(req.form.get('file', [''])[0])
    if not path:
        content = web.manifest(tmpl, web.changectx(req), path)
        req.respond(HTTP_OK, web.ctype)
        return content

    try:
        fctx = web.filectx(req)
    except revlog.LookupError, inst:
        try:
            content = web.manifest(tmpl, web.changectx(req), path)
            req.respond(HTTP_OK, web.ctype)
            return content
        except ErrorResponse:
            raise inst

    path = fctx.path()
    text = fctx.data()
    mt = mimetypes.guess_type(path)[0]
    if mt is None or util.binary(text):
        mt = mt or 'application/octet-stream'

    req.respond(HTTP_OK, mt, path, len(text))
    return [text]

def file(web, req, tmpl):
    path = web.cleanpath(req.form.get('file', [''])[0])
    if path:
        try:
            return web.filerevision(tmpl, web.filectx(req))
        except revlog.LookupError, inst:
            pass

    try:
        return web.manifest(tmpl, web.changectx(req), path)
    except ErrorResponse:
        raise inst

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
        except RepoError:
            return web.search(tmpl, hi) # XXX redirect to 404 page?

    return web.changelog(tmpl, ctx, shortlog = shortlog)

def shortlog(web, req, tmpl):
    return changelog(web, req, tmpl, shortlog = True)

def changeset(web, req, tmpl):
    return web.changeset(tmpl, web.changectx(req))

rev = changeset

def manifest(web, req, tmpl):
    return web.manifest(tmpl, web.changectx(req),
                        web.cleanpath(req.form['path'][0]))

def tags(web, req, tmpl):
    return web.tags(tmpl)

def summary(web, req, tmpl):
    return web.summary(tmpl)

def filediff(web, req, tmpl):
    return web.filediff(tmpl, web.filectx(req))

diff = filediff

def annotate(web, req, tmpl):
    return web.fileannotate(tmpl, web.filectx(req))

def filelog(web, req, tmpl):
    return web.filelog(tmpl, web.filectx(req))

def archive(web, req, tmpl):
    type_ = req.form['type'][0]
    allowed = web.configlist("web", "allow_archive")
    if (type_ in web.archives and (type_ in allowed or
        web.configbool("web", "allow" + type_, False))):
        web.archive(tmpl, req, req.form['node'][0], type_)
        return []
    raise ErrorResponse(HTTP_NOT_FOUND, 'unsupported archive type: %s' % type_)

def static(web, req, tmpl):
    fname = req.form['file'][0]
    # a repo owner may set web.static in .hg/hgrc to get any file
    # readable by the user running the CGI script
    static = web.config("web", "static",
                        os.path.join(web.templatepath, "static"),
                        untrusted=False)
    return [staticfile(static, fname, req)]
