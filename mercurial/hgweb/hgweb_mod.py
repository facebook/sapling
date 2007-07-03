# hgweb/hgweb_mod.py - Web interface for a repository.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, mimetypes, re, zlib, mimetools, cStringIO, sys
import tempfile, urllib, bz2
from mercurial.node import *
from mercurial.i18n import gettext as _
from mercurial import mdiff, ui, hg, util, archival, streamclone, patch
from mercurial import revlog, templater
from common import get_mtime, staticfile, style_map, paritygen

def _up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def revnavgen(pos, pagelen, limit, nodefunc):
    def seq(factor, limit=None):
        if limit:
            yield limit
            if limit >= 20 and limit <= 40:
                yield 50
        else:
            yield 1 * factor
            yield 3 * factor
        for f in seq(factor * 10):
            yield f

    def nav(**map):
        l = []
        last = 0
        for f in seq(1, pagelen):
            if f < pagelen or f <= last:
                continue
            if f > limit:
                break
            last = f
            if pos + f < limit:
                l.append(("+%d" % f, hex(nodefunc(pos + f).node())))
            if pos - f >= 0:
                l.insert(0, ("-%d" % f, hex(nodefunc(pos - f).node())))

        try:
            yield {"label": "(0)", "node": hex(nodefunc('0').node())}

            for label, node in l:
                yield {"label": label, "node": node}

            yield {"label": "tip", "node": "tip"}
        except hg.RepoError:
            pass

    return nav

class hgweb(object):
    def __init__(self, repo, name=None):
        if type(repo) == type(""):
            self.repo = hg.repository(ui.ui(report_untrusted=False), repo)
        else:
            self.repo = repo

        self.mtime = -1
        self.reponame = name
        self.archives = 'zip', 'gz', 'bz2'
        self.stripecount = 1
        # a repo owner may set web.templates in .hg/hgrc to get any file
        # readable by the user running the CGI script
        self.templatepath = self.config("web", "templates",
                                        templater.templatepath(),
                                        untrusted=False)

    # The CGI scripts are often run by a user different from the repo owner.
    # Trust the settings from the .hg/hgrc files by default.
    def config(self, section, name, default=None, untrusted=True):
        return self.repo.ui.config(section, name, default,
                                   untrusted=untrusted)

    def configbool(self, section, name, default=False, untrusted=True):
        return self.repo.ui.configbool(section, name, default,
                                       untrusted=untrusted)

    def configlist(self, section, name, default=None, untrusted=True):
        return self.repo.ui.configlist(section, name, default,
                                       untrusted=untrusted)

    def refresh(self):
        mtime = get_mtime(self.repo.root)
        if mtime != self.mtime:
            self.mtime = mtime
            self.repo = hg.repository(self.repo.ui, self.repo.root)
            self.maxchanges = int(self.config("web", "maxchanges", 10))
            self.stripecount = int(self.config("web", "stripes", 1))
            self.maxshortchanges = int(self.config("web", "maxshortchanges", 60))
            self.maxfiles = int(self.config("web", "maxfiles", 10))
            self.allowpull = self.configbool("web", "allowpull", True)
            self.encoding = self.config("web", "encoding", util._encoding)

    def archivelist(self, nodeid):
        allowed = self.configlist("web", "allow_archive")
        for i, spec in self.archive_specs.iteritems():
            if i in allowed or self.configbool("web", "allow" + i):
                yield {"type" : i, "extension" : spec[2], "node" : nodeid}

    def listfilediffs(self, files, changeset):
        for f in files[:self.maxfiles]:
            yield self.t("filedifflink", node=hex(changeset), file=f)
        if len(files) > self.maxfiles:
            yield self.t("fileellipses")

    def siblings(self, siblings=[], hiderev=None, **args):
        siblings = [s for s in siblings if s.node() != nullid]
        if len(siblings) == 1 and siblings[0].rev() == hiderev:
            return
        for s in siblings:
            d = {'node': hex(s.node()), 'rev': s.rev()}
            if hasattr(s, 'path'):
                d['file'] = s.path()
            d.update(args)
            yield d

    def renamelink(self, fl, node):
        r = fl.renamed(node)
        if r:
            return [dict(file=r[0], node=hex(r[1]))]
        return []

    def nodetagsdict(self, node):
        return [{"name": i} for i in self.repo.nodetags(node)]

    def nodebranchdict(self, ctx):
        branches = []
        branch = ctx.branch()
        if self.repo.branchtags()[branch] == ctx.node():
            branches.append({"name": branch})
        return branches

    def showtag(self, t1, node=nullid, **args):
        for t in self.repo.nodetags(node):
            yield self.t(t1, tag=t, **args)

    def diff(self, node1, node2, files):
        def filterfiles(filters, files):
            l = [x for x in files if x in filters]

            for t in filters:
                if t and t[-1] != os.sep:
                    t += os.sep
                l += [x for x in files if x.startswith(t)]
            return l

        parity = paritygen(self.stripecount)
        def diffblock(diff, f, fn):
            yield self.t("diffblock",
                         lines=prettyprintlines(diff),
                         parity=parity.next(),
                         file=f,
                         filenode=hex(fn or nullid))

        def prettyprintlines(diff):
            for l in diff.splitlines(1):
                if l.startswith('+'):
                    yield self.t("difflineplus", line=l)
                elif l.startswith('-'):
                    yield self.t("difflineminus", line=l)
                elif l.startswith('@'):
                    yield self.t("difflineat", line=l)
                else:
                    yield self.t("diffline", line=l)

        r = self.repo
        c1 = r.changectx(node1)
        c2 = r.changectx(node2)
        date1 = util.datestr(c1.date())
        date2 = util.datestr(c2.date())

        modified, added, removed, deleted, unknown = r.status(node1, node2)[:5]
        if files:
            modified, added, removed = map(lambda x: filterfiles(files, x),
                                           (modified, added, removed))

        diffopts = patch.diffopts(self.repo.ui, untrusted=True)
        for f in modified:
            to = c1.filectx(f).data()
            tn = c2.filectx(f).data()
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)
        for f in added:
            to = None
            tn = c2.filectx(f).data()
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)
        for f in removed:
            to = c1.filectx(f).data()
            tn = None
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)

    def changelog(self, ctx, shortlog=False):
        def changelist(**map):
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in xrange(start, end):
                ctx = self.repo.changectx(i)
                n = ctx.node()

                l.insert(0, {"parity": parity.next(),
                             "author": ctx.user(),
                             "parent": self.siblings(ctx.parents(), i - 1),
                             "child": self.siblings(ctx.children(), i + 1),
                             "changelogtag": self.showtag("changelogtag",n),
                             "desc": ctx.description(),
                             "date": ctx.date(),
                             "files": self.listfilediffs(ctx.files(), n),
                             "rev": i,
                             "node": hex(n),
                             "tags": self.nodetagsdict(n),
                             "branches": self.nodebranchdict(ctx)})

            for e in l:
                yield e

        maxchanges = shortlog and self.maxshortchanges or self.maxchanges
        cl = self.repo.changelog
        count = cl.count()
        pos = ctx.rev()
        start = max(0, pos - maxchanges + 1)
        end = min(count, start + maxchanges)
        pos = end - 1
        parity = paritygen(self.stripecount, offset=start-end)

        changenav = revnavgen(pos, maxchanges, count, self.repo.changectx)

        yield self.t(shortlog and 'shortlog' or 'changelog',
                     changenav=changenav,
                     node=hex(cl.tip()),
                     rev=pos, changesets=count, entries=changelist,
                     archives=self.archivelist("tip"))

    def search(self, query):

        def changelist(**map):
            cl = self.repo.changelog
            count = 0
            qw = query.lower().split()

            def revgen():
                for i in xrange(cl.count() - 1, 0, -100):
                    l = []
                    for j in xrange(max(0, i - 100), i):
                        ctx = self.repo.changectx(j)
                        l.append(ctx)
                    l.reverse()
                    for e in l:
                        yield e

            for ctx in revgen():
                miss = 0
                for q in qw:
                    if not (q in ctx.user().lower() or
                            q in ctx.description().lower() or
                            q in " ".join(ctx.files()).lower()):
                        miss = 1
                        break
                if miss:
                    continue

                count += 1
                n = ctx.node()

                yield self.t('searchentry',
                             parity=parity.next(),
                             author=ctx.user(),
                             parent=self.siblings(ctx.parents()),
                             child=self.siblings(ctx.children()),
                             changelogtag=self.showtag("changelogtag",n),
                             desc=ctx.description(),
                             date=ctx.date(),
                             files=self.listfilediffs(ctx.files(), n),
                             rev=ctx.rev(),
                             node=hex(n),
                             tags=self.nodetagsdict(n),
                             branches=self.nodebranchdict(ctx))

                if count >= self.maxchanges:
                    break

        cl = self.repo.changelog
        parity = paritygen(self.stripecount)

        yield self.t('search',
                     query=query,
                     node=hex(cl.tip()),
                     entries=changelist,
                     archives=self.archivelist("tip"))

    def changeset(self, ctx):
        n = ctx.node()
        parents = ctx.parents()
        p1 = parents[0].node()

        files = []
        parity = paritygen(self.stripecount)
        for f in ctx.files():
            files.append(self.t("filenodelink",
                                node=hex(n), file=f,
                                parity=parity.next()))

        def diff(**map):
            yield self.diff(p1, n, None)

        yield self.t('changeset',
                     diff=diff,
                     rev=ctx.rev(),
                     node=hex(n),
                     parent=self.siblings(parents),
                     child=self.siblings(ctx.children()),
                     changesettag=self.showtag("changesettag",n),
                     author=ctx.user(),
                     desc=ctx.description(),
                     date=ctx.date(),
                     files=files,
                     archives=self.archivelist(hex(n)),
                     tags=self.nodetagsdict(n),
                     branches=self.nodebranchdict(ctx))

    def filelog(self, fctx):
        f = fctx.path()
        fl = fctx.filelog()
        count = fl.count()
        pagelen = self.maxshortchanges
        pos = fctx.filerev()
        start = max(0, pos - pagelen + 1)
        end = min(count, start + pagelen)
        pos = end - 1
        parity = paritygen(self.stripecount, offset=start-end)

        def entries(**map):
            l = []

            for i in xrange(start, end):
                ctx = fctx.filectx(i)
                n = fl.node(i)

                l.insert(0, {"parity": parity.next(),
                             "filerev": i,
                             "file": f,
                             "node": hex(ctx.node()),
                             "author": ctx.user(),
                             "date": ctx.date(),
                             "rename": self.renamelink(fl, n),
                             "parent": self.siblings(fctx.parents()),
                             "child": self.siblings(fctx.children()),
                             "desc": ctx.description()})

            for e in l:
                yield e

        nodefunc = lambda x: fctx.filectx(fileid=x)
        nav = revnavgen(pos, pagelen, count, nodefunc)
        yield self.t("filelog", file=f, node=hex(fctx.node()), nav=nav,
                     entries=entries)

    def filerevision(self, fctx):
        f = fctx.path()
        text = fctx.data()
        fl = fctx.filelog()
        n = fctx.filenode()
        parity = paritygen(self.stripecount)

        mt = mimetypes.guess_type(f)[0]
        rawtext = text
        if util.binary(text):
            mt = mt or 'application/octet-stream'
            text = "(binary:%s)" % mt
        mt = mt or 'text/plain'

        def lines():
            for l, t in enumerate(text.splitlines(1)):
                yield {"line": t,
                       "linenumber": "% 6d" % (l + 1),
                       "parity": parity.next()}

        yield self.t("filerevision",
                     file=f,
                     path=_up(f),
                     text=lines(),
                     raw=rawtext,
                     mimetype=mt,
                     rev=fctx.rev(),
                     node=hex(fctx.node()),
                     author=fctx.user(),
                     date=fctx.date(),
                     desc=fctx.description(),
                     parent=self.siblings(fctx.parents()),
                     child=self.siblings(fctx.children()),
                     rename=self.renamelink(fl, n),
                     permissions=fctx.manifest().flags(f))

    def fileannotate(self, fctx):
        f = fctx.path()
        n = fctx.filenode()
        fl = fctx.filelog()
        parity = paritygen(self.stripecount)

        def annotate(**map):
            last = None
            for f, l in fctx.annotate(follow=True):
                fnode = f.filenode()
                name = self.repo.ui.shortuser(f.user())

                if last != fnode:
                    last = fnode

                yield {"parity": parity.next(),
                       "node": hex(f.node()),
                       "rev": f.rev(),
                       "author": name,
                       "file": f.path(),
                       "line": l}

        yield self.t("fileannotate",
                     file=f,
                     annotate=annotate,
                     path=_up(f),
                     rev=fctx.rev(),
                     node=hex(fctx.node()),
                     author=fctx.user(),
                     date=fctx.date(),
                     desc=fctx.description(),
                     rename=self.renamelink(fl, n),
                     parent=self.siblings(fctx.parents()),
                     child=self.siblings(fctx.children()),
                     permissions=fctx.manifest().flags(f))

    def manifest(self, ctx, path):
        mf = ctx.manifest()
        node = ctx.node()

        files = {}
        parity = paritygen(self.stripecount)

        if path and path[-1] != "/":
            path += "/"
        l = len(path)
        abspath = "/" + path

        for f, n in mf.items():
            if f[:l] != path:
                continue
            remain = f[l:]
            if "/" in remain:
                short = remain[:remain.index("/") + 1] # bleah
                files[short] = (f, None)
            else:
                short = os.path.basename(remain)
                files[short] = (f, n)

        def filelist(**map):
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if not fnode:
                    continue

                yield {"file": full,
                       "parity": parity.next(),
                       "basename": f,
                       "size": ctx.filectx(full).size(),
                       "permissions": mf.flags(full)}

        def dirlist(**map):
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if fnode:
                    continue

                yield {"parity": parity.next(),
                       "path": os.path.join(abspath, f),
                       "basename": f[:-1]}

        yield self.t("manifest",
                     rev=ctx.rev(),
                     node=hex(node),
                     path=abspath,
                     up=_up(abspath),
                     upparity=parity.next(),
                     fentries=filelist,
                     dentries=dirlist,
                     archives=self.archivelist(hex(node)),
                     tags=self.nodetagsdict(node),
                     branches=self.nodebranchdict(ctx))

    def tags(self):
        i = self.repo.tagslist()
        i.reverse()
        parity = paritygen(self.stripecount)

        def entries(notip=False, **map):
            for k, n in i:
                if notip and k == "tip":
                    continue
                yield {"parity": parity.next(),
                       "tag": k,
                       "date": self.repo.changectx(n).date(),
                       "node": hex(n)}

        yield self.t("tags",
                     node=hex(self.repo.changelog.tip()),
                     entries=lambda **x: entries(False, **x),
                     entriesnotip=lambda **x: entries(True, **x))

    def summary(self):
        i = self.repo.tagslist()
        i.reverse()

        def tagentries(**map):
            parity = paritygen(self.stripecount)
            count = 0
            for k, n in i:
                if k == "tip": # skip tip
                    continue;

                count += 1
                if count > 10: # limit to 10 tags
                    break;

                yield self.t("tagentry",
                             parity=parity.next(),
                             tag=k,
                             node=hex(n),
                             date=self.repo.changectx(n).date())


        def branches(**map):
            parity = paritygen(self.stripecount)

            b = self.repo.branchtags()
            l = [(-self.repo.changelog.rev(n), n, t) for t, n in b.items()]
            l.sort()

            for r,n,t in l:
                ctx = self.repo.changectx(n)

                yield {'parity': parity.next(),
                       'branch': t,
                       'node': hex(n),
                       'date': ctx.date()}

        def changelist(**map):
            parity = paritygen(self.stripecount, offset=start-end)
            l = [] # build a list in forward order for efficiency
            for i in xrange(start, end):
                ctx = self.repo.changectx(i)
                n = ctx.node()
                hn = hex(n)

                l.insert(0, self.t(
                    'shortlogentry',
                    parity=parity.next(),
                    author=ctx.user(),
                    desc=ctx.description(),
                    date=ctx.date(),
                    rev=i,
                    node=hn,
                    tags=self.nodetagsdict(n),
                    branches=self.nodebranchdict(ctx)))

            yield l

        cl = self.repo.changelog
        count = cl.count()
        start = max(0, count - self.maxchanges)
        end = min(count, start + self.maxchanges)

        yield self.t("summary",
                 desc=self.config("web", "description", "unknown"),
                 owner=(self.config("ui", "username") or # preferred
                        self.config("web", "contact") or # deprecated
                        self.config("web", "author", "unknown")), # also
                 lastchange=cl.read(cl.tip())[2],
                 tags=tagentries,
                 branches=branches,
                 shortlog=changelist,
                 node=hex(cl.tip()),
                 archives=self.archivelist("tip"))

    def filediff(self, fctx):
        n = fctx.node()
        path = fctx.path()
        parents = fctx.parents()
        p1 = parents and parents[0].node() or nullid

        def diff(**map):
            yield self.diff(p1, n, [path])

        yield self.t("filediff",
                     file=path,
                     node=hex(n),
                     rev=fctx.rev(),
                     parent=self.siblings(parents),
                     child=self.siblings(fctx.children()),
                     diff=diff)

    archive_specs = {
        'bz2': ('application/x-tar', 'tbz2', '.tar.bz2', None),
        'gz': ('application/x-tar', 'tgz', '.tar.gz', None),
        'zip': ('application/zip', 'zip', '.zip', None),
        }

    def archive(self, req, key, type_):
        reponame = re.sub(r"\W+", "-", os.path.basename(self.reponame))
        cnode = self.repo.lookup(key)
        arch_version = key
        if cnode == key or key == 'tip':
            arch_version = short(cnode)
        name = "%s-%s" % (reponame, arch_version)
        mimetype, artype, extension, encoding = self.archive_specs[type_]
        headers = [('Content-type', mimetype),
                   ('Content-disposition', 'attachment; filename=%s%s' %
                    (name, extension))]
        if encoding:
            headers.append(('Content-encoding', encoding))
        req.header(headers)
        archival.archive(self.repo, req.out, cnode, artype, prefix=name)

    # add tags to things
    # tags -> list of changesets corresponding to tags
    # find tag, changeset, file

    def cleanpath(self, path):
        path = path.lstrip('/')
        return util.canonpath(self.repo.root, '', path)

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
                ''.join(self.t("header", encoding=self.encoding, **map)))
            msg = mimetools.Message(header_file, 0)
            req.header(msg.items())
            yield header_file.read()

        def rawfileheader(**map):
            req.header([('Content-type', map['mimetype']),
                        ('Content-disposition', 'filename=%s' % map['file']),
                        ('Content-length', str(len(map['raw'])))])
            yield ''

        def footer(**map):
            yield self.t("footer", **map)

        def motd(**map):
            yield self.config("web", "motd", "")

        def expand_form(form):
            shortcuts = {
                'cl': [('cmd', ['changelog']), ('rev', None)],
                'sl': [('cmd', ['shortlog']), ('rev', None)],
                'cs': [('cmd', ['changeset']), ('node', None)],
                'f': [('cmd', ['file']), ('filenode', None)],
                'fl': [('cmd', ['filelog']), ('filenode', None)],
                'fd': [('cmd', ['filediff']), ('node', None)],
                'fa': [('cmd', ['annotate']), ('filenode', None)],
                'mf': [('cmd', ['manifest']), ('manifest', None)],
                'ca': [('cmd', ['archive']), ('node', None)],
                'tags': [('cmd', ['tags'])],
                'tip': [('cmd', ['changeset']), ('node', ['tip'])],
                'static': [('cmd', ['static']), ('file', None)]
            }

            for k in shortcuts.iterkeys():
                if form.has_key(k):
                    for name, value in shortcuts[k]:
                        if value is None:
                            value = form[k]
                        form[name] = value
                    del form[k]

        def rewrite_request(req):
            '''translate new web interface to traditional format'''

            def spliturl(req):
                def firstitem(query):
                    return query.split('&', 1)[0].split(';', 1)[0]

                def normurl(url):
                    inner = '/'.join([x for x in url.split('/') if x])
                    tl = len(url) > 1 and url.endswith('/') and '/' or ''

                    return '%s%s%s' % (url.startswith('/') and '/' or '',
                                       inner, tl)

                root = normurl(urllib.unquote(req.env.get('REQUEST_URI', '').split('?', 1)[0]))
                pi = normurl(req.env.get('PATH_INFO', ''))
                if pi:
                    # strip leading /
                    pi = pi[1:]
                    if pi:
                        root = root[:root.rfind(pi)]
                    if req.env.has_key('REPO_NAME'):
                        rn = req.env['REPO_NAME'] + '/'
                        root += rn
                        query = pi[len(rn):]
                    else:
                        query = pi
                else:
                    root += '?'
                    query = firstitem(req.env['QUERY_STRING'])

                return (root, query)

            req.url, query = spliturl(req)

            if req.form.has_key('cmd'):
                # old style
                return

            args = query.split('/', 2)
            if not args or not args[0]:
                return

            cmd = args.pop(0)
            style = cmd.rfind('-')
            if style != -1:
                req.form['style'] = [cmd[:style]]
                cmd = cmd[style+1:]
            # avoid accepting e.g. style parameter as command
            if hasattr(self, 'do_' + cmd):
                req.form['cmd'] = [cmd]

            if args and args[0]:
                node = args.pop(0)
                req.form['node'] = [node]
            if args:
                req.form['file'] = args

            if cmd == 'static':
                req.form['file'] = req.form['node']
            elif cmd == 'archive':
                fn = req.form['node'][0]
                for type_, spec in self.archive_specs.iteritems():
                    ext = spec[2]
                    if fn.endswith(ext):
                        req.form['node'] = [fn[:-len(ext)]]
                        req.form['type'] = [type_]

        def sessionvars(**map):
            fields = []
            if req.form.has_key('style'):
                style = req.form['style'][0]
                if style != self.config('web', 'style', ''):
                    fields.append(('style', style))

            separator = req.url[-1] == '?' and ';' or '?'
            for name, value in fields:
                yield dict(name=name, value=value, separator=separator)
                separator = ';'

        self.refresh()

        expand_form(req.form)
        rewrite_request(req)

        style = self.config("web", "style", "")
        if req.form.has_key('style'):
            style = req.form['style'][0]
        mapfile = style_map(self.templatepath, style)

        port = req.env["SERVER_PORT"]
        port = port != "80" and (":" + port) or ""
        urlbase = 'http://%s%s' % (req.env['SERVER_NAME'], port)
        staticurl = self.config("web", "staticurl") or req.url + 'static/'
        if not staticurl.endswith('/'):
            staticurl += '/'

        if not self.reponame:
            self.reponame = (self.config("web", "name")
                             or req.env.get('REPO_NAME')
                             or req.url.strip('/') or self.repo.root)

        self.t = templater.templater(mapfile, templater.common_filters,
                                     defaults={"url": req.url,
                                               "staticurl": staticurl,
                                               "urlbase": urlbase,
                                               "repo": self.reponame,
                                               "header": header,
                                               "footer": footer,
                                               "motd": motd,
                                               "rawfileheader": rawfileheader,
                                               "sessionvars": sessionvars
                                               })

        try:
            if not req.form.has_key('cmd'):
                req.form['cmd'] = [self.t.cache['default']]

            cmd = req.form['cmd'][0]

            method = getattr(self, 'do_' + cmd, None)
            if method:
                try:
                    method(req)
                except (hg.RepoError, revlog.RevlogError), inst:
                    req.write(self.t("error", error=str(inst)))
            else:
                req.write(self.t("error", error='No such method: ' + cmd))
        finally:
            self.t = None

    def changectx(self, req):
        if req.form.has_key('node'):
            changeid = req.form['node'][0]
        elif req.form.has_key('manifest'):
            changeid = req.form['manifest'][0]
        else:
            changeid = self.repo.changelog.count() - 1

        try:
            ctx = self.repo.changectx(changeid)
        except hg.RepoError:
            man = self.repo.manifest
            mn = man.lookup(changeid)
            ctx = self.repo.changectx(man.linkrev(mn))

        return ctx

    def filectx(self, req):
        path = self.cleanpath(req.form['file'][0])
        if req.form.has_key('node'):
            changeid = req.form['node'][0]
        else:
            changeid = req.form['filenode'][0]
        try:
            ctx = self.repo.changectx(changeid)
            fctx = ctx.filectx(path)
        except hg.RepoError:
            fctx = self.repo.filectx(path, fileid=changeid)

        return fctx

    def do_log(self, req):
        if req.form.has_key('file') and req.form['file'][0]:
            self.do_filelog(req)
        else:
            self.do_changelog(req)

    def do_rev(self, req):
        self.do_changeset(req)

    def do_file(self, req):
        path = self.cleanpath(req.form.get('file', [''])[0])
        if path:
            try:
                req.write(self.filerevision(self.filectx(req)))
                return
            except revlog.LookupError:
                pass

        req.write(self.manifest(self.changectx(req), path))

    def do_diff(self, req):
        self.do_filediff(req)

    def do_changelog(self, req, shortlog = False):
        if req.form.has_key('node'):
            ctx = self.changectx(req)
        else:
            if req.form.has_key('rev'):
                hi = req.form['rev'][0]
            else:
                hi = self.repo.changelog.count() - 1
            try:
                ctx = self.repo.changectx(hi)
            except hg.RepoError:
                req.write(self.search(hi)) # XXX redirect to 404 page?
                return

        req.write(self.changelog(ctx, shortlog = shortlog))

    def do_shortlog(self, req):
        self.do_changelog(req, shortlog = True)

    def do_changeset(self, req):
        req.write(self.changeset(self.changectx(req)))

    def do_manifest(self, req):
        req.write(self.manifest(self.changectx(req),
                                self.cleanpath(req.form['path'][0])))

    def do_tags(self, req):
        req.write(self.tags())

    def do_summary(self, req):
        req.write(self.summary())

    def do_filediff(self, req):
        req.write(self.filediff(self.filectx(req)))

    def do_annotate(self, req):
        req.write(self.fileannotate(self.filectx(req)))

    def do_filelog(self, req):
        req.write(self.filelog(self.filectx(req)))

    def do_lookup(self, req):
        try:
            r = hex(self.repo.lookup(req.form['key'][0]))
            success = 1
        except Exception,inst:
            r = str(inst)
            success = 0
        resp = "%s %s\n" % (success, r)
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def do_heads(self, req):
        resp = " ".join(map(hex, self.repo.heads())) + "\n"
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def do_branches(self, req):
        nodes = []
        if req.form.has_key('nodes'):
            nodes = map(bin, req.form['nodes'][0].split(" "))
        resp = cStringIO.StringIO()
        for b in self.repo.branches(nodes):
            resp.write(" ".join(map(hex, b)) + "\n")
        resp = resp.getvalue()
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def do_between(self, req):
        if req.form.has_key('pairs'):
            pairs = [map(bin, p.split("-"))
                     for p in req.form['pairs'][0].split(" ")]
        resp = cStringIO.StringIO()
        for b in self.repo.between(pairs):
            resp.write(" ".join(map(hex, b)) + "\n")
        resp = resp.getvalue()
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def do_changegroup(self, req):
        req.httphdr("application/mercurial-0.1")
        nodes = []
        if not self.allowpull:
            return

        if req.form.has_key('roots'):
            nodes = map(bin, req.form['roots'][0].split(" "))

        z = zlib.compressobj()
        f = self.repo.changegroup(nodes, 'serve')
        while 1:
            chunk = f.read(4096)
            if not chunk:
                break
            req.write(z.compress(chunk))

        req.write(z.flush())

    def do_changegroupsubset(self, req):
        req.httphdr("application/mercurial-0.1")
        bases = []
        heads = []
        if not self.allowpull:
            return

        if req.form.has_key('bases'):
            bases = [bin(x) for x in req.form['bases'][0].split(' ')]
        if req.form.has_key('heads'):
            heads = [bin(x) for x in req.form['heads'][0].split(' ')]

        z = zlib.compressobj()
        f = self.repo.changegroupsubset(bases, heads, 'serve')
        while 1:
            chunk = f.read(4096)
            if not chunk:
                break
            req.write(z.compress(chunk))

        req.write(z.flush())

    def do_archive(self, req):
        type_ = req.form['type'][0]
        allowed = self.configlist("web", "allow_archive")
        if (type_ in self.archives and (type_ in allowed or
            self.configbool("web", "allow" + type_, False))):
            self.archive(req, req.form['node'][0], type_)
            return

        req.write(self.t("error"))

    def do_static(self, req):
        fname = req.form['file'][0]
        # a repo owner may set web.static in .hg/hgrc to get any file
        # readable by the user running the CGI script
        static = self.config("web", "static",
                             os.path.join(self.templatepath, "static"),
                             untrusted=False)
        req.write(staticfile(static, fname, req)
                  or self.t("error", error="%r not found" % fname))

    def do_capabilities(self, req):
        caps = ['lookup', 'changegroupsubset']
        if self.configbool('server', 'uncompressed'):
            caps.append('stream=%d' % self.repo.changelog.version)
        # XXX: make configurable and/or share code with do_unbundle:
        unbundleversions = ['HG10GZ', 'HG10BZ', 'HG10UN']
        if unbundleversions:
            caps.append('unbundle=%s' % ','.join(unbundleversions))
        resp = ' '.join(caps)
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def check_perm(self, req, op, default):
        '''check permission for operation based on user auth.
        return true if op allowed, else false.
        default is policy to use if no config given.'''

        user = req.env.get('REMOTE_USER')

        deny = self.configlist('web', 'deny_' + op)
        if deny and (not user or deny == ['*'] or user in deny):
            return False

        allow = self.configlist('web', 'allow_' + op)
        return (allow and (allow == ['*'] or user in allow)) or default

    def do_unbundle(self, req):
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
        ssl_req = self.configbool('web', 'push_ssl', True)
        if ssl_req:
            if not req.env.get('HTTPS'):
                bail(_('ssl required\n'))
                return
            proto = 'https'
        else:
            proto = 'http'

        # do not allow push unless explicitly allowed
        if not self.check_perm(req, 'push', False):
            bail(_('push not authorized\n'),
                 headers={'status': '401 Unauthorized'})
            return

        their_heads = req.form['heads'][0].split(' ')

        def check_heads():
            heads = map(hex, self.repo.heads())
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
                lock = self.repo.lock()
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
                            ret = self.repo.addchangegroup(
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
                    lock.release()
            except (OSError, IOError), inst:
                req.write('0\n')
                filename = getattr(inst, 'filename', '')
                # Don't send our filesystem layout to the client
                if filename.startswith(self.repo.root):
                    filename = filename[len(self.repo.root)+1:]
                else:
                    filename = ''
                error = getattr(inst, 'strerror', 'Unknown error')
                req.write('%s: %s\n' % (error, filename))
        finally:
            fp.close()
            os.unlink(tempname)

    def do_stream_out(self, req):
        req.httphdr("application/mercurial-0.1")
        streamclone.stream_out(self.repo, req)
