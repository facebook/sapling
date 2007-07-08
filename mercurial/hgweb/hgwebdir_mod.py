# hgweb/hgwebdir_mod.py - Web interface for a directory of repositories.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial import demandimport; demandimport.enable()
import os, mimetools, cStringIO
from mercurial.i18n import gettext as _
from mercurial import ui, hg, util, templater
from common import get_mtime, staticfile, style_map, paritygen
from hgweb_mod import hgweb

# This is a stopgap
class hgwebdir(object):
    def __init__(self, config, parentui=None):
        def cleannames(items):
            return [(name.strip(os.sep), path) for name, path in items]

        self.parentui = parentui
        self.motd = None
        self.style = None
        self.stripecount = None
        self.repos_sorted = ('name', False)
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
            if cp.has_section('paths'):
                self.repos.extend(cleannames(cp.items('paths')))
            if cp.has_section('collections'):
                for prefix, root in cp.items('collections'):
                    for path in util.walkrepos(root):
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
        from request import wsgiapplication
        def make_web_app():
            return self
        wsgicgi.launch(wsgiapplication(make_web_app))

    def run_wsgi(self, req):
        def header(**map):
            header_file = cStringIO.StringIO(
                ''.join(tmpl("header", encoding=util._encoding, **map)))
            msg = mimetools.Message(header_file, 0)
            req.header(msg.items())
            yield header_file.read()

        def footer(**map):
            yield tmpl("footer", **map)

        def motd(**map):
            if self.motd is not None:
                yield self.motd
            else:
                yield config('web', 'motd', '')

        parentui = self.parentui or ui.ui(report_untrusted=False)

        def config(section, name, default=None, untrusted=True):
            return parentui.config(section, name, default, untrusted)

        url = req.env['REQUEST_URI'].split('?')[0]
        if not url.endswith('/'):
            url += '/'
        pathinfo = req.env.get('PATH_INFO', '').strip('/') + '/'
        base = url[:len(url) - len(pathinfo)]
        if not base.endswith('/'):
            base += '/'

        staticurl = config('web', 'staticurl') or base + 'static/'
        if not staticurl.endswith('/'):
            staticurl += '/'

        style = self.style
        if style is None:
            style = config('web', 'style', '')
        if req.form.has_key('style'):
            style = req.form['style'][0]
        if self.stripecount is None:
            self.stripecount = int(config('web', 'stripes', 1))
        mapfile = style_map(templater.templatepath(), style)
        tmpl = templater.templater(mapfile, templater.common_filters,
                                   defaults={"header": header,
                                             "footer": footer,
                                             "motd": motd,
                                             "url": url,
                                             "staticurl": staticurl})

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
                if req.form.has_key('style'):
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

                u = ui.ui(parentui=parentui)
                try:
                    u.readconfig(os.path.join(path, '.hg', 'hgrc'))
                except IOError:
                    pass
                def get(section, name, default=None):
                    return u.config(section, name, default, untrusted=True)

                if u.configbool("web", "hidden", untrusted=True):
                    continue

                url = ('/'.join([req.env["REQUEST_URI"].split('?')[0], name])
                       .replace("//", "/")) + '/'

                # update time with local timezone
                try:
                    d = (get_mtime(path), util.makedate()[1])
                except OSError:
                    continue

                contact = (get("ui", "username") or # preferred
                           get("web", "contact") or # deprecated
                           get("web", "author", "")) # also
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

        def makeindex(req, subdir=""):
            sortable = ["name", "description", "contact", "lastchange"]
            sortcolumn, descending = self.repos_sorted
            if req.form.has_key('sort'):
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
            req.write(tmpl("index", entries=entries, subdir=subdir,
                           sortcolumn=sortcolumn, descending=descending,
                           **dict(sort)))

        try:
            virtual = req.env.get("PATH_INFO", "").strip('/')
            if virtual.startswith('static/'):
                static = os.path.join(templater.templatepath(), 'static')
                fname = virtual[7:]
                req.write(staticfile(static, fname, req) or
                          tmpl('error', error='%r not found' % fname))
            elif virtual:
                repos = dict(self.repos)
                while virtual:
                    real = repos.get(virtual)
                    if real:
                        req.env['REPO_NAME'] = virtual
                        try:
                            repo = hg.repository(parentui, real)
                            hgweb(repo).run_wsgi(req)
                        except IOError, inst:
                            req.write(tmpl("error", error=inst.strerror))
                        except hg.RepoError, inst:
                            req.write(tmpl("error", error=str(inst)))
                        return

                    # browse subdirectories
                    subdir = virtual + '/'
                    if [r for r in repos if r.startswith(subdir)]:
                        makeindex(req, subdir)
                        return

                    up = virtual.rfind('/')
                    if up < 0:
                        break
                    virtual = virtual[:up]
                
                req.write(tmpl("notfound", repo=virtual))
            else:
                if req.form.has_key('static'):
                    static = os.path.join(templater.templatepath(), "static")
                    fname = req.form['static'][0]
                    req.write(staticfile(static, fname, req)
                              or tmpl("error", error="%r not found" % fname))
                else:
                    makeindex(req)
        finally:
            tmpl = None
