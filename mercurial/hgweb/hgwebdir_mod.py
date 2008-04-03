# hgweb/hgwebdir_mod.py - Web interface for a directory of repositories.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, mimetools, cStringIO
from mercurial.i18n import gettext as _
from mercurial.repo import RepoError
from mercurial import ui, hg, util, templater, templatefilters
from common import ErrorResponse, get_mtime, staticfile, style_map, paritygen,\
                   get_contact, HTTP_OK, HTTP_NOT_FOUND, HTTP_SERVER_ERROR
from hgweb_mod import hgweb
from request import wsgirequest

# This is a stopgap
class hgwebdir(object):
    def __init__(self, config, parentui=None):
        def cleannames(items):
            return [(util.pconvert(name).strip('/'), path)
                    for name, path in items]

        self.parentui = parentui or ui.ui(report_untrusted=False,
                                          interactive = False)
        self.motd = None
        self.style = None
        self.stripecount = None
        self.repos_sorted = ('name', False)
        self._baseurl = None
        if isinstance(config, (list, tuple)):
            self.repos = cleannames(config)
            self.repos_sorted = ('', False)
        elif isinstance(config, dict):
            self.repos = cleannames(config.items())
            self.repos.sort()
        else:
            if isinstance(config, util.configparser):
                cp = config
            else:
                cp = util.configparser()
                cp.read(config)
            self.repos = []
            if cp.has_section('web'):
                if cp.has_option('web', 'motd'):
                    self.motd = cp.get('web', 'motd')
                if cp.has_option('web', 'style'):
                    self.style = cp.get('web', 'style')
                if cp.has_option('web', 'stripes'):
                    self.stripecount = int(cp.get('web', 'stripes'))
                if cp.has_option('web', 'baseurl'):
                    self._baseurl = cp.get('web', 'baseurl')
            if cp.has_section('paths'):
                self.repos.extend(cleannames(cp.items('paths')))
            if cp.has_section('collections'):
                for prefix, root in cp.items('collections'):
                    for path in util.walkrepos(root, followsym=True):
                        repo = os.path.normpath(path)
                        name = repo
                        if name.startswith(prefix):
                            name = name[len(prefix):]
                        self.repos.append((name.lstrip(os.sep), repo))
            self.repos.sort()

    def run(self):
        if not os.environ.get('GATEWAY_INTERFACE', '').startswith("CGI/1."):
            raise RuntimeError("This function is only intended to be called while running as a CGI script.")
        import mercurial.hgweb.wsgicgi as wsgicgi
        wsgicgi.launch(self)

    def __call__(self, env, respond):
        req = wsgirequest(env, respond)
        self.run_wsgi(req)
        return req

    def run_wsgi(self, req):

        try:
            try:

                virtual = req.env.get("PATH_INFO", "").strip('/')
                tmpl = self.templater(req)
                try:
                    ctype = tmpl('mimetype', encoding=util._encoding)
                    ctype = templater.stringify(ctype)
                except KeyError:
                    # old templates with inline HTTP headers?
                    if 'mimetype' in tmpl:
                        raise
                    header = tmpl('header', encoding=util._encoding)
                    header_file = cStringIO.StringIO(templater.stringify(header))
                    msg = mimetools.Message(header_file, 0)
                    ctype = msg['content-type']

                # a static file
                if virtual.startswith('static/') or 'static' in req.form:
                    static = os.path.join(templater.templatepath(), 'static')
                    if virtual.startswith('static/'):
                        fname = virtual[7:]
                    else:
                        fname = req.form['static'][0]
                    req.write(staticfile(static, fname, req))
                    return

                # top-level index
                elif not virtual:
                    req.respond(HTTP_OK, ctype)
                    req.write(self.makeindex(req, tmpl))
                    return

                # nested indexes and hgwebs

                repos = dict(self.repos)
                while virtual:
                    real = repos.get(virtual)
                    if real:
                        req.env['REPO_NAME'] = virtual
                        try:
                            repo = hg.repository(self.parentui, real)
                            hgweb(repo).run_wsgi(req)
                            return
                        except IOError, inst:
                            msg = inst.strerror
                            raise ErrorResponse(HTTP_SERVER_ERROR, msg)
                        except RepoError, inst:
                            raise ErrorResponse(HTTP_SERVER_ERROR, str(inst))

                    # browse subdirectories
                    subdir = virtual + '/'
                    if [r for r in repos if r.startswith(subdir)]:
                        req.respond(HTTP_OK, ctype)
                        req.write(self.makeindex(req, tmpl, subdir))
                        return

                    up = virtual.rfind('/')
                    if up < 0:
                        break
                    virtual = virtual[:up]

                # prefixes not found
                req.respond(HTTP_NOT_FOUND, ctype)
                req.write(tmpl("notfound", repo=virtual))

            except ErrorResponse, err:
                req.respond(err.code, ctype)
                req.write(tmpl('error', error=err.message or ''))
        finally:
            tmpl = None

    def makeindex(self, req, tmpl, subdir=""):

        def archivelist(ui, nodeid, url):
            allowed = ui.configlist("web", "allow_archive", untrusted=True)
            for i in [('zip', '.zip'), ('gz', '.tar.gz'), ('bz2', '.tar.bz2')]:
                if i[0] in allowed or ui.configbool("web", "allow" + i[0],
                                                    untrusted=True):
                    yield {"type" : i[0], "extension": i[1],
                           "node": nodeid, "url": url}

        def entries(sortcolumn="", descending=False, subdir="", **map):
            def sessionvars(**map):
                fields = []
                if 'style' in req.form:
                    style = req.form['style'][0]
                    if style != get('web', 'style', ''):
                        fields.append(('style', style))

                separator = url[-1] == '?' and ';' or '?'
                for name, value in fields:
                    yield dict(name=name, value=value, separator=separator)
                    separator = ';'

            rows = []
            parity = paritygen(self.stripecount)
            for name, path in self.repos:
                if not name.startswith(subdir):
                    continue
                name = name[len(subdir):]

                u = ui.ui(parentui=self.parentui)
                try:
                    u.readconfig(os.path.join(path, '.hg', 'hgrc'))
                except Exception, e:
                    u.warn(_('error reading %s/.hg/hgrc: %s\n' % (path, e)))
                    continue
                def get(section, name, default=None):
                    return u.config(section, name, default, untrusted=True)

                if u.configbool("web", "hidden", untrusted=True):
                    continue

                parts = [name]
                if 'PATH_INFO' in req.env:
                    parts.insert(0, req.env['PATH_INFO'].rstrip('/'))
                if req.env['SCRIPT_NAME']:
                    parts.insert(0, req.env['SCRIPT_NAME'])
                url = ('/'.join(parts).replace("//", "/")) + '/'

                # update time with local timezone
                try:
                    d = (get_mtime(path), util.makedate()[1])
                except OSError:
                    continue

                contact = get_contact(get)
                description = get("web", "description", "")
                name = get("web", "name", name)
                row = dict(contact=contact or "unknown",
                           contact_sort=contact.upper() or "unknown",
                           name=name,
                           name_sort=name,
                           url=url,
                           description=description or "unknown",
                           description_sort=description.upper() or "unknown",
                           lastchange=d,
                           lastchange_sort=d[1]-d[0],
                           sessionvars=sessionvars,
                           archives=archivelist(u, "tip", url))
                if (not sortcolumn
                    or (sortcolumn, descending) == self.repos_sorted):
                    # fast path for unsorted output
                    row['parity'] = parity.next()
                    yield row
                else:
                    rows.append((row["%s_sort" % sortcolumn], row))
            if rows:
                rows.sort()
                if descending:
                    rows.reverse()
                for key, row in rows:
                    row['parity'] = parity.next()
                    yield row

        sortable = ["name", "description", "contact", "lastchange"]
        sortcolumn, descending = self.repos_sorted
        if 'sort' in req.form:
            sortcolumn = req.form['sort'][0]
            descending = sortcolumn.startswith('-')
            if descending:
                sortcolumn = sortcolumn[1:]
            if sortcolumn not in sortable:
                sortcolumn = ""

        sort = [("sort_%s" % column,
                 "%s%s" % ((not descending and column == sortcolumn)
                            and "-" or "", column))
                for column in sortable]

        if self._baseurl is not None:
            req.env['SCRIPT_NAME'] = self._baseurl

        return tmpl("index", entries=entries, subdir=subdir,
                    sortcolumn=sortcolumn, descending=descending,
                    **dict(sort))

    def templater(self, req):

        def header(**map):
            header = tmpl('header', encoding=util._encoding, **map)
            if 'mimetype' not in tmpl:
                # old template with inline HTTP headers
                header_file = cStringIO.StringIO(templater.stringify(header))
                msg = mimetools.Message(header_file, 0)
                header = header_file.read()
            yield header

        def footer(**map):
            yield tmpl("footer", **map)

        def motd(**map):
            if self.motd is not None:
                yield self.motd
            else:
                yield config('web', 'motd', '')

        def config(section, name, default=None, untrusted=True):
            return self.parentui.config(section, name, default, untrusted)

        if self._baseurl is not None:
            req.env['SCRIPT_NAME'] = self._baseurl

        url = req.env.get('SCRIPT_NAME', '')
        if not url.endswith('/'):
            url += '/'

        staticurl = config('web', 'staticurl') or url + 'static/'
        if not staticurl.endswith('/'):
            staticurl += '/'

        style = self.style
        if style is None:
            style = config('web', 'style', '')
        if 'style' in req.form:
            style = req.form['style'][0]
        if self.stripecount is None:
            self.stripecount = int(config('web', 'stripes', 1))
        mapfile = style_map(templater.templatepath(), style)
        tmpl = templater.templater(mapfile, templatefilters.filters,
                                   defaults={"header": header,
                                             "footer": footer,
                                             "motd": motd,
                                             "url": url,
                                             "staticurl": staticurl})
        return tmpl
