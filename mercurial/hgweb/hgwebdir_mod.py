# hgweb/hgwebdir_mod.py - Web interface for a directory of repositories.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import re
import time

from ..i18n import _

from .common import (
    ErrorResponse,
    HTTP_NOT_FOUND,
    HTTP_OK,
    HTTP_SERVER_ERROR,
    cspvalues,
    get_contact,
    get_mtime,
    ismember,
    paritygen,
    staticfile,
)
from .request import wsgirequest

from .. import (
    configitems,
    encoding,
    error,
    hg,
    profiling,
    pycompat,
    scmutil,
    templater,
    ui as uimod,
    util,
)

from . import (
    hgweb_mod,
    webutil,
    wsgicgi,
)

def cleannames(items):
    return [(util.pconvert(name).strip('/'), path) for name, path in items]

def findrepos(paths):
    repos = []
    for prefix, root in cleannames(paths):
        roothead, roottail = os.path.split(root)
        # "foo = /bar/*" or "foo = /bar/**" lets every repo /bar/N in or below
        # /bar/ be served as as foo/N .
        # '*' will not search inside dirs with .hg (except .hg/patches),
        # '**' will search inside dirs with .hg (and thus also find subrepos).
        try:
            recurse = {'*': False, '**': True}[roottail]
        except KeyError:
            repos.append((prefix, root))
            continue
        roothead = os.path.normpath(os.path.abspath(roothead))
        paths = scmutil.walkrepos(roothead, followsym=True, recurse=recurse)
        repos.extend(urlrepos(prefix, roothead, paths))
    return repos

def urlrepos(prefix, roothead, paths):
    """yield url paths and filesystem paths from a list of repo paths

    >>> conv = lambda seq: [(v, util.pconvert(p)) for v,p in seq]
    >>> conv(urlrepos(b'hg', b'/opt', [b'/opt/r', b'/opt/r/r', b'/opt']))
    [('hg/r', '/opt/r'), ('hg/r/r', '/opt/r/r'), ('hg', '/opt')]
    >>> conv(urlrepos(b'', b'/opt', [b'/opt/r', b'/opt/r/r', b'/opt']))
    [('r', '/opt/r'), ('r/r', '/opt/r/r'), ('', '/opt')]
    """
    for path in paths:
        path = os.path.normpath(path)
        yield (prefix + '/' +
               util.pconvert(path[len(roothead):]).lstrip('/')).strip('/'), path

def geturlcgivars(baseurl, port):
    """
    Extract CGI variables from baseurl

    >>> geturlcgivars(b"http://host.org/base", b"80")
    ('host.org', '80', '/base')
    >>> geturlcgivars(b"http://host.org:8000/base", b"80")
    ('host.org', '8000', '/base')
    >>> geturlcgivars(b'/base', 8000)
    ('', '8000', '/base')
    >>> geturlcgivars(b"base", b'8000')
    ('', '8000', '/base')
    >>> geturlcgivars(b"http://host", b'8000')
    ('host', '8000', '/')
    >>> geturlcgivars(b"http://host/", b'8000')
    ('host', '8000', '/')
    """
    u = util.url(baseurl)
    name = u.host or ''
    if u.port:
        port = u.port
    path = u.path or ""
    if not path.startswith('/'):
        path = '/' + path

    return name, pycompat.bytestr(port), path

class hgwebdir(object):
    """HTTP server for multiple repositories.

    Given a configuration, different repositories will be served depending
    on the request path.

    Instances are typically used as WSGI applications.
    """
    def __init__(self, conf, baseui=None):
        self.conf = conf
        self.baseui = baseui
        self.ui = None
        self.lastrefresh = 0
        self.motd = None
        self.refresh()

    def refresh(self):
        if self.ui:
            refreshinterval = self.ui.configint('web', 'refreshinterval')
        else:
            item = configitems.coreitems['web']['refreshinterval']
            refreshinterval = item.default

        # refreshinterval <= 0 means to always refresh.
        if (refreshinterval > 0 and
            self.lastrefresh + refreshinterval > time.time()):
            return

        if self.baseui:
            u = self.baseui.copy()
        else:
            u = uimod.ui.load()
            u.setconfig('ui', 'report_untrusted', 'off', 'hgwebdir')
            u.setconfig('ui', 'nontty', 'true', 'hgwebdir')
            # displaying bundling progress bar while serving feels wrong and may
            # break some wsgi implementations.
            u.setconfig('progress', 'disable', 'true', 'hgweb')

        if not isinstance(self.conf, (dict, list, tuple)):
            map = {'paths': 'hgweb-paths'}
            if not os.path.exists(self.conf):
                raise error.Abort(_('config file %s not found!') % self.conf)
            u.readconfig(self.conf, remap=map, trust=True)
            paths = []
            for name, ignored in u.configitems('hgweb-paths'):
                for path in u.configlist('hgweb-paths', name):
                    paths.append((name, path))
        elif isinstance(self.conf, (list, tuple)):
            paths = self.conf
        elif isinstance(self.conf, dict):
            paths = self.conf.items()

        repos = findrepos(paths)
        for prefix, root in u.configitems('collections'):
            prefix = util.pconvert(prefix)
            for path in scmutil.walkrepos(root, followsym=True):
                repo = os.path.normpath(path)
                name = util.pconvert(repo)
                if name.startswith(prefix):
                    name = name[len(prefix):]
                repos.append((name.lstrip('/'), repo))

        self.repos = repos
        self.ui = u
        encoding.encoding = self.ui.config('web', 'encoding')
        self.style = self.ui.config('web', 'style')
        self.templatepath = self.ui.config('web', 'templates', untrusted=False)
        self.stripecount = self.ui.config('web', 'stripes')
        if self.stripecount:
            self.stripecount = int(self.stripecount)
        self._baseurl = self.ui.config('web', 'baseurl')
        prefix = self.ui.config('web', 'prefix')
        if prefix.startswith('/'):
            prefix = prefix[1:]
        if prefix.endswith('/'):
            prefix = prefix[:-1]
        self.prefix = prefix
        self.lastrefresh = time.time()

    def run(self):
        if not encoding.environ.get('GATEWAY_INTERFACE',
                                    '').startswith("CGI/1."):
            raise RuntimeError("This function is only intended to be "
                               "called while running as a CGI script.")
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
        if deny_read and (not user or ismember(ui, user, deny_read)):
            return False

        allow_read = ui.configlist('web', 'allow_read', untrusted=True)
        # by default, allow reading if no allow_read option has been set
        if (not allow_read) or ismember(ui, user, allow_read):
            return True

        return False

    def run_wsgi(self, req):
        profile = self.ui.configbool('profiling', 'enabled')
        with profiling.profile(self.ui, enabled=profile):
            for r in self._runwsgi(req):
                yield r

    def _runwsgi(self, req):
        try:
            self.refresh()

            csp, nonce = cspvalues(self.ui)
            if csp:
                req.headers.append(('Content-Security-Policy', csp))

            virtual = req.env.get("PATH_INFO", "").strip('/')
            tmpl = self.templater(req, nonce)
            ctype = tmpl('mimetype', encoding=encoding.encoding)
            ctype = templater.stringify(ctype)

            # a static file
            if virtual.startswith('static/') or 'static' in req.form:
                if virtual.startswith('static/'):
                    fname = virtual[7:]
                else:
                    fname = req.form['static'][0]
                static = self.ui.config("web", "static", None,
                                        untrusted=False)
                if not static:
                    tp = self.templatepath or templater.templatepaths()
                    if isinstance(tp, str):
                        tp = [tp]
                    static = [os.path.join(p, 'static') for p in tp]
                staticfile(static, fname, req)
                return []

            # top-level index

            repos = dict(self.repos)

            if (not virtual or virtual == 'index') and virtual not in repos:
                req.respond(HTTP_OK, ctype)
                return self.makeindex(req, tmpl)

            # nested indexes and hgwebs

            if virtual.endswith('/index') and virtual not in repos:
                subdir = virtual[:-len('index')]
                if any(r.startswith(subdir) for r in repos):
                    req.respond(HTTP_OK, ctype)
                    return self.makeindex(req, tmpl, subdir)

            def _virtualdirs():
                # Check the full virtual path, each parent, and the root ('')
                if virtual != '':
                    yield virtual

                    for p in util.finddirs(virtual):
                        yield p

                yield ''

            for virtualrepo in _virtualdirs():
                real = repos.get(virtualrepo)
                if real:
                    req.env['REPO_NAME'] = virtualrepo
                    try:
                        # ensure caller gets private copy of ui
                        repo = hg.repository(self.ui.copy(), real)
                        return hgweb_mod.hgweb(repo).run_wsgi(req)
                    except IOError as inst:
                        msg = encoding.strtolocal(inst.strerror)
                        raise ErrorResponse(HTTP_SERVER_ERROR, msg)
                    except error.RepoError as inst:
                        raise ErrorResponse(HTTP_SERVER_ERROR, bytes(inst))

            # browse subdirectories
            subdir = virtual + '/'
            if [r for r in repos if r.startswith(subdir)]:
                req.respond(HTTP_OK, ctype)
                return self.makeindex(req, tmpl, subdir)

            # prefixes not found
            req.respond(HTTP_NOT_FOUND, ctype)
            return tmpl("notfound", repo=virtual)

        except ErrorResponse as err:
            req.respond(err, ctype)
            return tmpl('error', error=err.message or '')
        finally:
            tmpl = None

    def makeindex(self, req, tmpl, subdir=""):

        def archivelist(ui, nodeid, url):
            allowed = ui.configlist("web", "allow_archive", untrusted=True)
            archives = []
            for typ, spec in hgweb_mod.archivespecs.iteritems():
                if typ in allowed or ui.configbool("web", "allow" + typ,
                                                    untrusted=True):
                    archives.append({"type": typ, "extension": spec[2],
                                     "node": nodeid, "url": url})
            return archives

        def rawentries(subdir="", **map):

            descend = self.ui.configbool('web', 'descend')
            collapse = self.ui.configbool('web', 'collapse')
            seenrepos = set()
            seendirs = set()
            for name, path in self.repos:

                if not name.startswith(subdir):
                    continue
                name = name[len(subdir):]
                directory = False

                if '/' in name:
                    if not descend:
                        continue

                    nameparts = name.split('/')
                    rootname = nameparts[0]

                    if not collapse:
                        pass
                    elif rootname in seendirs:
                        continue
                    elif rootname in seenrepos:
                        pass
                    else:
                        directory = True
                        name = rootname

                        # redefine the path to refer to the directory
                        discarded = '/'.join(nameparts[1:])

                        # remove name parts plus accompanying slash
                        path = path[:-len(discarded) - 1]

                        try:
                            r = hg.repository(self.ui, path)
                            directory = False
                        except (IOError, error.RepoError):
                            pass

                parts = [name]
                parts.insert(0, '/' + subdir.rstrip('/'))
                if req.env['SCRIPT_NAME']:
                    parts.insert(0, req.env['SCRIPT_NAME'])
                url = re.sub(r'/+', '/', '/'.join(parts) + '/')

                # show either a directory entry or a repository
                if directory:
                    # get the directory's time information
                    try:
                        d = (get_mtime(path), util.makedate()[1])
                    except OSError:
                        continue

                    # add '/' to the name to make it obvious that
                    # the entry is a directory, not a regular repository
                    row = {'contact': "",
                           'contact_sort': "",
                           'name': name + '/',
                           'name_sort': name,
                           'url': url,
                           'description': "",
                           'description_sort': "",
                           'lastchange': d,
                           'lastchange_sort': d[1]-d[0],
                           'archives': [],
                           'isdirectory': True,
                           'labels': [],
                           }

                    seendirs.add(name)
                    yield row
                    continue

                u = self.ui.copy()
                try:
                    u.readconfig(os.path.join(path, '.hg', 'hgrc'))
                except Exception as e:
                    u.warn(_('error reading %s/.hg/hgrc: %s\n') % (path, e))
                    continue
                def get(section, name, default=uimod._unset):
                    return u.config(section, name, default, untrusted=True)

                if u.configbool("web", "hidden", untrusted=True):
                    continue

                if not self.read_allowed(u, req):
                    continue

                # update time with local timezone
                try:
                    r = hg.repository(self.ui, path)
                except IOError:
                    u.warn(_('error accessing repository at %s\n') % path)
                    continue
                except error.RepoError:
                    u.warn(_('error accessing repository at %s\n') % path)
                    continue
                try:
                    d = (get_mtime(r.spath), util.makedate()[1])
                except OSError:
                    continue

                contact = get_contact(get)
                description = get("web", "description")
                seenrepos.add(name)
                name = get("web", "name", name)
                row = {'contact': contact or "unknown",
                       'contact_sort': contact.upper() or "unknown",
                       'name': name,
                       'name_sort': name,
                       'url': url,
                       'description': description or "unknown",
                       'description_sort': description.upper() or "unknown",
                       'lastchange': d,
                       'lastchange_sort': d[1]-d[0],
                       'archives': archivelist(u, "tip", url),
                       'isdirectory': None,
                       'labels': u.configlist('web', 'labels', untrusted=True),
                       }

                yield row

        sortdefault = None, False
        def entries(sortcolumn="", descending=False, subdir="", **map):
            rows = rawentries(subdir=subdir, **map)

            if sortcolumn and sortdefault != (sortcolumn, descending):
                sortkey = '%s_sort' % sortcolumn
                rows = sorted(rows, key=lambda x: x[sortkey],
                              reverse=descending)
            for row, parity in zip(rows, paritygen(self.stripecount)):
                row['parity'] = parity
                yield row

        self.refresh()
        sortable = ["name", "description", "contact", "lastchange"]
        sortcolumn, descending = sortdefault
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

        self.refresh()
        self.updatereqenv(req.env)

        return tmpl("index", entries=entries, subdir=subdir,
                    pathdef=hgweb_mod.makebreadcrumb('/' + subdir, self.prefix),
                    sortcolumn=sortcolumn, descending=descending,
                    **dict(sort))

    def templater(self, req, nonce):

        def motd(**map):
            if self.motd is not None:
                yield self.motd
            else:
                yield config('web', 'motd')

        def config(section, name, default=uimod._unset, untrusted=True):
            return self.ui.config(section, name, default, untrusted)

        self.updatereqenv(req.env)

        url = req.env.get('SCRIPT_NAME', '')
        if not url.endswith('/'):
            url += '/'

        vars = {}
        styles, (style, mapfile) = hgweb_mod.getstyle(req, config,
                                                      self.templatepath)
        if style == styles[0]:
            vars['style'] = style

        start = r'&' if url[-1] == r'?' else r'?'
        sessionvars = webutil.sessionvars(vars, start)
        logourl = config('web', 'logourl')
        logoimg = config('web', 'logoimg')
        staticurl = config('web', 'staticurl') or url + 'static/'
        if not staticurl.endswith('/'):
            staticurl += '/'

        defaults = {
            "encoding": encoding.encoding,
            "motd": motd,
            "url": url,
            "logourl": logourl,
            "logoimg": logoimg,
            "staticurl": staticurl,
            "sessionvars": sessionvars,
            "style": style,
            "nonce": nonce,
        }
        tmpl = templater.templater.frommapfile(mapfile, defaults=defaults)
        return tmpl

    def updatereqenv(self, env):
        if self._baseurl is not None:
            name, port, path = geturlcgivars(self._baseurl, env['SERVER_PORT'])
            env['SERVER_NAME'] = name
            env['SERVER_PORT'] = port
            env['SCRIPT_NAME'] = path
