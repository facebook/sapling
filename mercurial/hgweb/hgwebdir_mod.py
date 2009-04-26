# hgweb/hgwebdir_mod.py - Web interface for a directory of repositories.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
from mercurial.i18n import _
from mercurial import ui, hg, util, templater, templatefilters
from mercurial import config, error, encoding
from common import ErrorResponse, get_mtime, staticfile, paritygen,\
                   get_contact, HTTP_OK, HTTP_NOT_FOUND, HTTP_SERVER_ERROR
from hgweb_mod import hgweb
from request import wsgirequest

# This is a stopgap
class hgwebdir(object):
    def __init__(self, conf, parentui=None):
        def cleannames(items):
            return [(util.pconvert(name).strip('/'), path)
                    for name, path in items]

        if parentui:
           self.parentui = parentui
        else:
            self.parentui = ui.ui()
            self.parentui.setconfig('ui', 'report_untrusted', 'off')
            self.parentui.setconfig('ui', 'interactive', 'off')

        self.motd = None
        self.style = 'paper'
        self.stripecount = None
        self.repos_sorted = ('name', False)
        self._baseurl = None
        if isinstance(conf, (list, tuple)):
            self.repos = cleannames(conf)
            self.repos_sorted = ('', False)
        elif isinstance(conf, dict):
            self.repos = util.sort(cleannames(conf.items()))
        else:
            if isinstance(conf, config.config):
                cp = conf
            else:
                cp = config.config()
                cp.read(conf)
            self.repos = []
            self.motd = cp.get('web', 'motd')
            self.style = cp.get('web', 'style', 'paper')
            self.stripecount = cp.get('web', 'stripes')
            self._baseurl = cp.get('web', 'baseurl')
            if 'paths' in cp:
                paths = cleannames(cp.items('paths'))
                for prefix, root in paths:
                    roothead, roottail = os.path.split(root)
                    # "foo = /bar/*" makes every subrepo of /bar/ to be
                    # mounted as foo/subrepo
                    # and "foo = /bar/**" does even recurse inside the
                    # subdirectories, remember to use it without working dir.
                    try:
                        recurse = {'*': False, '**': True}[roottail]
                    except KeyError:
                        self.repos.append((prefix, root))
                        continue
                    roothead = os.path.normpath(roothead)
                    for path in util.walkrepos(roothead, followsym=True,
                                               recurse=recurse):
                        path = os.path.normpath(path)
                        name = util.pconvert(path[len(roothead):]).strip('/')
                        if prefix:
                            name = prefix + '/' + name
                        self.repos.append((name, path))
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
        return self.run_wsgi(req)

    def read_allowed(self, ui, req):
        """Check allow_read and deny_read config options of a repo's ui object
        to determine user permissions.  By default, with neither option set (or
        both empty), allow all users to read the repo.  There are two ways a
        user can be denied read access:  (1) deny_read is not empty, and the
        user is unauthenticated or deny_read contains user (or *), and (2)
        allow_read is not empty and the user is not in allow_read.  Return True
        if user is allowed to read the repo, else return False."""

        user = req.env.get('REMOTE_USER')

        deny_read = ui.configlist('web', 'deny_read', untrusted=True)
        if deny_read and (not user or deny_read == ['*'] or user in deny_read):
            return False

        allow_read = ui.configlist('web', 'allow_read', untrusted=True)
        # by default, allow reading if no allow_read option has been set
        if (not allow_read) or (allow_read == ['*']) or (user in allow_read):
            return True

        return False

    def run_wsgi(self, req):

        try:
            try:

                virtual = req.env.get("PATH_INFO", "").strip('/')
                tmpl = self.templater(req)
                ctype = tmpl('mimetype', encoding=encoding.encoding)
                ctype = templater.stringify(ctype)

                # a static file
                if virtual.startswith('static/') or 'static' in req.form:
                    if virtual.startswith('static/'):
                        fname = virtual[7:]
                    else:
                        fname = req.form['static'][0]
                    static = templater.templatepath('static')
                    return (staticfile(static, fname, req),)

                # top-level index
                elif not virtual:
                    req.respond(HTTP_OK, ctype)
                    return self.makeindex(req, tmpl)

                # nested indexes and hgwebs

                repos = dict(self.repos)
                while virtual:
                    real = repos.get(virtual)
                    if real:
                        req.env['REPO_NAME'] = virtual
                        try:
                            repo = hg.repository(self.parentui, real)
                            return hgweb(repo).run_wsgi(req)
                        except IOError, inst:
                            msg = inst.strerror
                            raise ErrorResponse(HTTP_SERVER_ERROR, msg)
                        except error.RepoError, inst:
                            raise ErrorResponse(HTTP_SERVER_ERROR, str(inst))

                    # browse subdirectories
                    subdir = virtual + '/'
                    if [r for r in repos if r.startswith(subdir)]:
                        req.respond(HTTP_OK, ctype)
                        return self.makeindex(req, tmpl, subdir)

                    up = virtual.rfind('/')
                    if up < 0:
                        break
                    virtual = virtual[:up]

                # prefixes not found
                req.respond(HTTP_NOT_FOUND, ctype)
                return tmpl("notfound", repo=virtual)

            except ErrorResponse, err:
                req.respond(err, ctype)
                return tmpl('error', error=err.message or '')
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
                    u.warn(_('error reading %s/.hg/hgrc: %s\n') % (path, e))
                    continue
                def get(section, name, default=None):
                    return u.config(section, name, default, untrusted=True)

                if u.configbool("web", "hidden", untrusted=True):
                    continue

                if not self.read_allowed(u, req):
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
            yield tmpl('header', encoding=encoding.encoding, **map)

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
        self.stripecount = int(self.stripecount or config('web', 'stripes', 1))
        mapfile = templater.stylemap(style)
        tmpl = templater.templater(mapfile, templatefilters.filters,
                                   defaults={"header": header,
                                             "footer": footer,
                                             "motd": motd,
                                             "url": url,
                                             "staticurl": staticurl})
        return tmpl
