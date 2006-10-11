# hgweb/hgweb_mod.py - Web interface for a repository.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
import os.path
import mimetypes
from mercurial.demandload import demandload
demandload(globals(), "re zlib ConfigParser mimetools cStringIO sys tempfile")
demandload(globals(), 'urllib')
demandload(globals(), "mercurial:mdiff,ui,hg,util,archival,streamclone,patch")
demandload(globals(), "mercurial:revlog,templater")
demandload(globals(), "mercurial.hgweb.common:get_mtime,staticfile,style_map")
from mercurial.node import *
from mercurial.i18n import gettext as _

def _up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

class hgweb(object):
    def __init__(self, repo, name=None):
        if type(repo) == type(""):
            self.repo = hg.repository(ui.ui(), repo)
        else:
            self.repo = repo

        self.mtime = -1
        self.reponame = name
        self.archives = 'zip', 'gz', 'bz2'
        self.stripecount = 1
        self.templatepath = self.repo.ui.config("web", "templates",
                                                templater.templatepath())

    def refresh(self):
        mtime = get_mtime(self.repo.root)
        if mtime != self.mtime:
            self.mtime = mtime
            self.repo = hg.repository(self.repo.ui, self.repo.root)
            self.maxchanges = int(self.repo.ui.config("web", "maxchanges", 10))
            self.stripecount = int(self.repo.ui.config("web", "stripes", 1))
            self.maxshortchanges = int(self.repo.ui.config("web", "maxshortchanges", 60))
            self.maxfiles = int(self.repo.ui.config("web", "maxfiles", 10))
            self.allowpull = self.repo.ui.configbool("web", "allowpull", True)

    def archivelist(self, nodeid):
        allowed = self.repo.ui.configlist("web", "allow_archive")
        for i, spec in self.archive_specs.iteritems():
            if i in allowed or self.repo.ui.configbool("web", "allow" + i):
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
            yield dict(node=hex(s.node()), rev=s.rev(), **args)

    def renamelink(self, fl, node):
        r = fl.renamed(node)
        if r:
            return [dict(file=r[0], node=hex(r[1]))]
        return []

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

        parity = [0]
        def diffblock(diff, f, fn):
            yield self.t("diffblock",
                         lines=prettyprintlines(diff),
                         parity=parity[0],
                         file=f,
                         filenode=hex(fn or nullid))
            parity[0] = 1 - parity[0]

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
        cl = r.changelog
        mf = r.manifest
        change1 = cl.read(node1)
        change2 = cl.read(node2)
        mmap1 = mf.read(change1[0])
        mmap2 = mf.read(change2[0])
        date1 = util.datestr(change1[2])
        date2 = util.datestr(change2[2])

        modified, added, removed, deleted, unknown = r.status(node1, node2)[:5]
        if files:
            modified, added, removed = map(lambda x: filterfiles(files, x),
                                           (modified, added, removed))

        diffopts = patch.diffopts(self.repo.ui)
        for f in modified:
            to = r.file(f).read(mmap1[f])
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)
        for f in added:
            to = None
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)
        for f in removed:
            to = r.file(f).read(mmap1[f])
            tn = None
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                                          opts=diffopts), f, tn)

    def changelog(self, ctx, shortlog=False):
        pos = ctx.rev()
        def changenav(**map):
            def seq(factor, maxchanges=None):
                if maxchanges:
                    yield maxchanges
                    if maxchanges >= 20 and maxchanges <= 40:
                        yield 50
                else:
                    yield 1 * factor
                    yield 3 * factor
                for f in seq(factor * 10):
                    yield f

            l = []
            last = 0
            maxchanges = shortlog and self.maxshortchanges or self.maxchanges
            for f in seq(1, maxchanges):
                if f < maxchanges or f <= last:
                    continue
                if f > count:
                    break
                last = f
                r = "%d" % f
                if pos + f < count:
                    l.append(("+" + r, pos + f))
                if pos - f >= 0:
                    l.insert(0, ("-" + r, pos - f))

            yield {"rev": 0, "label": "(0)"}

            for label, rev in l:
                yield {"label": label, "rev": rev}

            yield {"label": "tip", "rev": "tip"}

        def changelist(**map):
            parity = (start - end) & 1
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in range(start, end):
                ctx = self.repo.changectx(i)
                n = ctx.node()

                l.insert(0, {"parity": parity,
                             "author": ctx.user(),
                             "parent": self.siblings(ctx.parents(), i - 1),
                             "child": self.siblings(ctx.children(), i + 1),
                             "changelogtag": self.showtag("changelogtag",n),
                             "desc": ctx.description(),
                             "date": ctx.date(),
                             "files": self.listfilediffs(ctx.files(), n),
                             "rev": i,
                             "node": hex(n)})
                parity = 1 - parity

            for e in l:
                yield e

        maxchanges = shortlog and self.maxshortchanges or self.maxchanges
        cl = self.repo.changelog
        count = cl.count()
        start = max(0, pos - maxchanges + 1)
        end = min(count, start + maxchanges)
        pos = end - 1

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
                for i in range(cl.count() - 1, 0, -100):
                    l = []
                    for j in range(max(0, i - 100), i):
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
                            q in " ".join(ctx.files()[:20]).lower()):
                        miss = 1
                        break
                if miss:
                    continue

                count += 1
                n = ctx.node()

                yield self.t('searchentry',
                             parity=self.stripes(count),
                             author=ctx.user(),
                             parent=self.siblings(ctx.parents()),
                             child=self.siblings(ctx.children()),
                             changelogtag=self.showtag("changelogtag",n),
                             desc=ctx.description(),
                             date=ctx.date(),
                             files=self.listfilediffs(ctx.files(), n),
                             rev=ctx.rev(),
                             node=hex(n))

                if count >= self.maxchanges:
                    break

        cl = self.repo.changelog

        yield self.t('search',
                     query=query,
                     node=hex(cl.tip()),
                     entries=changelist)

    def changeset(self, ctx):
        n = ctx.node()
        parents = ctx.parents()
        p1 = parents[0].node()

        files = []
        parity = 0
        for f in ctx.files():
            files.append(self.t("filenodelink",
                                node=hex(n), file=f,
                                parity=parity))
            parity = 1 - parity

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
                     archives=self.archivelist(hex(n)))

    def filelog(self, fctx):
        f = fctx.path()
        fl = fctx.filelog()
        count = fl.count()

        def entries(**map):
            l = []
            parity = (count - 1) & 1

            for i in range(count):
                ctx = fctx.filectx(i)
                n = fl.node(i)

                l.insert(0, {"parity": parity,
                             "filerev": i,
                             "file": f,
                             "node": hex(ctx.node()),
                             "author": ctx.user(),
                             "date": ctx.date(),
                             "rename": self.renamelink(fl, n),
                             "parent": self.siblings(fctx.parents(), file=f),
                             "child": self.siblings(fctx.children(), file=f),
                             "desc": ctx.description()})
                parity = 1 - parity

            for e in l:
                yield e

        yield self.t("filelog", file=f, node=hex(fctx.node()), entries=entries)

    def filerevision(self, fctx):
        f = fctx.path()
        text = fctx.data()
        fl = fctx.filelog()
        n = fctx.filenode()

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
                       "parity": self.stripes(l)}

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
                     parent=self.siblings(fctx.parents(), file=f),
                     child=self.siblings(fctx.children(), file=f),
                     rename=self.renamelink(fl, n),
                     permissions=fctx.manifest().execf(f))

    def fileannotate(self, fctx):
        f = fctx.path()
        n = fctx.filenode()
        fl = fctx.filelog()

        def annotate(**map):
            parity = 0
            last = None
            for f, l in fctx.annotate(follow=True):
                fnode = f.filenode()
                name = self.repo.ui.shortuser(f.user())

                if last != fnode:
                    parity = 1 - parity
                    last = fnode

                yield {"parity": parity,
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
                     rename=self.renamelink(fl, n),
                     parent=self.siblings(fctx.parents(), file=f),
                     child=self.siblings(fctx.children(), file=f),
                     permissions=fctx.manifest().execf(f))

    def manifest(self, ctx, path):
        mf = ctx.manifest()
        node = ctx.node()

        files = {}

        p = path[1:]
        if p and p[-1] != "/":
            p += "/"
        l = len(p)

        for f,n in mf.items():
            if f[:l] != p:
                continue
            remain = f[l:]
            if "/" in remain:
                short = remain[:remain.index("/") + 1] # bleah
                files[short] = (f, None)
            else:
                short = os.path.basename(remain)
                files[short] = (f, n)

        def filelist(**map):
            parity = 0
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if not fnode:
                    continue

                yield {"file": full,
                       "parity": self.stripes(parity),
                       "basename": f,
                       "size": ctx.filectx(full).size(),
                       "permissions": mf.execf(full)}
                parity += 1

        def dirlist(**map):
            parity = 0
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if fnode:
                    continue

                yield {"parity": self.stripes(parity),
                       "path": os.path.join(path, f),
                       "basename": f[:-1]}
                parity += 1

        yield self.t("manifest",
                     rev=ctx.rev(),
                     node=hex(node),
                     path=path,
                     up=_up(path),
                     fentries=filelist,
                     dentries=dirlist,
                     archives=self.archivelist(hex(node)))

    def tags(self):
        cl = self.repo.changelog

        i = self.repo.tagslist()
        i.reverse()

        def entries(notip=False, **map):
            parity = 0
            for k,n in i:
                if notip and k == "tip": continue
                yield {"parity": self.stripes(parity),
                       "tag": k,
                       "date": cl.read(n)[2],
                       "node": hex(n)}
                parity += 1

        yield self.t("tags",
                     node=hex(self.repo.changelog.tip()),
                     entries=lambda **x: entries(False, **x),
                     entriesnotip=lambda **x: entries(True, **x))

    def summary(self):
        cl = self.repo.changelog

        i = self.repo.tagslist()
        i.reverse()

        def tagentries(**map):
            parity = 0
            count = 0
            for k,n in i:
                if k == "tip": # skip tip
                    continue;

                count += 1
                if count > 10: # limit to 10 tags
                    break;

                c = cl.read(n)
                t = c[2]

                yield self.t("tagentry",
                             parity = self.stripes(parity),
                             tag = k,
                             node = hex(n),
                             date = t)
                parity += 1

        def changelist(**map):
            parity = 0
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in range(start, end):
                n = cl.node(i)
                changes = cl.read(n)
                hn = hex(n)
                t = changes[2]

                l.insert(0, self.t(
                    'shortlogentry',
                    parity = parity,
                    author = changes[1],
                    desc = changes[4],
                    date = t,
                    rev = i,
                    node = hn))
                parity = 1 - parity

            yield l

        count = cl.count()
        start = max(0, count - self.maxchanges)
        end = min(count, start + self.maxchanges)

        yield self.t("summary",
                 desc = self.repo.ui.config("web", "description", "unknown"),
                 owner = (self.repo.ui.config("ui", "username") or # preferred
                          self.repo.ui.config("web", "contact") or # deprecated
                          self.repo.ui.config("web", "author", "unknown")), # also
                 lastchange = cl.read(cl.tip())[2],
                 tags = tagentries,
                 shortlog = changelist,
                 node = hex(cl.tip()),
                 archives=self.archivelist("tip"))

    def filediff(self, fctx):
        n = fctx.node()
        path = fctx.path()
        parents = fctx.changectx().parents()
        p1 = parents[0].node()

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

    def archive(self, req, cnode, type_):
        reponame = re.sub(r"\W+", "-", os.path.basename(self.reponame))
        name = "%s-%s" % (reponame, short(cnode))
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
        p = util.normpath(path)
        if p[:2] == "..":
            raise Exception("suspicious path")
        return p

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
            header_file = cStringIO.StringIO(''.join(self.t("header", **map)))
            msg = mimetools.Message(header_file, 0)
            req.header(msg.items())
            yield header_file.read()

        def rawfileheader(**map):
            req.header([('Content-type', map['mimetype']),
                        ('Content-disposition', 'filename=%s' % map['file']),
                        ('Content-length', str(len(map['raw'])))])
            yield ''

        def footer(**map):
            yield self.t("footer",
                         motd=self.repo.ui.config("web", "motd", ""),
                         **map)

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

                root = normurl(req.env.get('REQUEST_URI', '').split('?', 1)[0])
                pi = normurl(req.env.get('PATH_INFO', ''))
                if pi:
                    # strip leading /
                    pi = pi[1:]
                    if pi:
                        root = root[:-len(pi)]
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

        def queryprefix(**map):
            return req.url[-1] == '?' and ';' or '?'

        def getentries(**map):
            fields = {}
            if req.form.has_key('style'):
                style = req.form['style'][0]
                if style != self.repo.ui.config('web', 'style', ''):
                    fields['style'] = style

            if fields:
                fields = ['%s=%s' % (k, urllib.quote(v))
                          for k, v in fields.iteritems()]
                yield '%s%s' % (queryprefix(), ';'.join(fields))
            else:
                yield ''

        self.refresh()

        expand_form(req.form)
        rewrite_request(req)

        style = self.repo.ui.config("web", "style", "")
        if req.form.has_key('style'):
            style = req.form['style'][0]
        mapfile = style_map(self.templatepath, style)

        if not req.url:
            port = req.env["SERVER_PORT"]
            port = port != "80" and (":" + port) or ""
            uri = req.env["REQUEST_URI"]
            if "?" in uri:
                uri = uri.split("?")[0]
            req.url = "http://%s%s%s" % (req.env["SERVER_NAME"], port, uri)

        if not self.reponame:
            self.reponame = (self.repo.ui.config("web", "name")
                             or req.env.get('REPO_NAME')
                             or req.url.strip('/') or self.repo.root)

        self.t = templater.templater(mapfile, templater.common_filters,
                                     defaults={"url": req.url,
                                               "repo": self.reponame,
                                               "header": header,
                                               "footer": footer,
                                               "rawfileheader": rawfileheader,
                                               "queryprefix": queryprefix,
                                               "getentries": getentries
                                               })

        if not req.form.has_key('cmd'):
            req.form['cmd'] = [self.t.cache['default'],]

        cmd = req.form['cmd'][0]

        method = getattr(self, 'do_' + cmd, None)
        if method:
            try:
                method(req)
            except (hg.RepoError, revlog.RevlogError), inst:
                req.write(self.t("error", error=str(inst)))
        else:
            req.write(self.t("error", error='No such method: ' + cmd))

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

    def stripes(self, parity):
        "make horizontal stripes for easier reading"
        if self.stripecount:
            return (1 + parity / self.stripecount) & 1
        else:
            return 0

    def do_log(self, req):
        if req.form.has_key('file') and req.form['file'][0]:
            self.do_filelog(req)
        else:
            self.do_changelog(req)

    def do_rev(self, req):
        self.do_changeset(req)

    def do_file(self, req):
        path = req.form.get('file', [''])[0]
        if path:
            try:
                req.write(self.filerevision(self.filectx(req)))
                return
            except hg.RepoError:
                pass
            path = self.cleanpath(path)

        req.write(self.manifest(self.changectx(req), '/' + path))

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

    def do_archive(self, req):
        changeset = self.repo.lookup(req.form['node'][0])
        type_ = req.form['type'][0]
        allowed = self.repo.ui.configlist("web", "allow_archive")
        if (type_ in self.archives and (type_ in allowed or
            self.repo.ui.configbool("web", "allow" + type_, False))):
            self.archive(req, changeset, type_)
            return

        req.write(self.t("error"))

    def do_static(self, req):
        fname = req.form['file'][0]
        static = self.repo.ui.config("web", "static",
                                     os.path.join(self.templatepath,
                                                  "static"))
        req.write(staticfile(static, fname, req)
                  or self.t("error", error="%r not found" % fname))

    def do_capabilities(self, req):
        caps = ['unbundle']
        if self.repo.ui.configbool('server', 'uncompressed'):
            caps.append('stream=%d' % self.repo.revlogversion)
        resp = ' '.join(caps)
        req.httphdr("application/mercurial-0.1", length=len(resp))
        req.write(resp)

    def check_perm(self, req, op, default):
        '''check permission for operation based on user auth.
        return true if op allowed, else false.
        default is policy to use if no config given.'''

        user = req.env.get('REMOTE_USER')

        deny = self.repo.ui.configlist('web', 'deny_' + op)
        if deny and (not user or deny == ['*'] or user in deny):
            return False

        allow = self.repo.ui.configlist('web', 'allow_' + op)
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
        ssl_req = self.repo.ui.configbool('web', 'push_ssl', True)
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

        req.httphdr("application/mercurial-0.1")

        their_heads = req.form['heads'][0].split(' ')

        def check_heads():
            heads = map(hex, self.repo.heads())
            return their_heads == [hex('force')] or their_heads == heads

        # fail early if possible
        if not check_heads():
            bail(_('unsynced changes\n'))
            return

        # do not lock repo until all changegroup data is
        # streamed. save to temporary file.

        fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
        fp = os.fdopen(fd, 'wb+')
        try:
            length = int(req.env['CONTENT_LENGTH'])
            for s in util.filechunkiter(req, limit=length):
                fp.write(s)

            lock = self.repo.lock()
            try:
                if not check_heads():
                    req.write('0\n')
                    req.write(_('unsynced changes\n'))
                    return

                fp.seek(0)

                # send addchangegroup output to client

                old_stdout = sys.stdout
                sys.stdout = cStringIO.StringIO()

                try:
                    url = 'remote:%s:%s' % (proto,
                                            req.env.get('REMOTE_HOST', ''))
                    ret = self.repo.addchangegroup(fp, 'serve', url)
                finally:
                    val = sys.stdout.getvalue()
                    sys.stdout = old_stdout
                req.write('%d\n' % ret)
                req.write(val)
            finally:
                lock.release()
        finally:
            fp.close()
            os.unlink(tempname)

    def do_stream_out(self, req):
        req.httphdr("application/mercurial-0.1")
        streamclone.stream_out(self.repo, req)
