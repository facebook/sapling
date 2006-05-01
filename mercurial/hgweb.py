# hgweb.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, cgi, sys
import mimetypes
from demandload import demandload
demandload(globals(), "mdiff time re socket zlib errno ui hg ConfigParser")
demandload(globals(), "tempfile StringIO BaseHTTPServer util SocketServer")
demandload(globals(), "archival mimetypes templater urllib")
from node import *
from i18n import gettext as _

def splitURI(uri):
    """ Return path and query splited from uri

    Just like CGI environment, the path is unquoted, the query is
    not.
    """
    if '?' in uri:
        path, query = uri.split('?', 1)
    else:
        path, query = uri, ''
    return urllib.unquote(path), query

def up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def get_mtime(repo_path):
    hg_path = os.path.join(repo_path, ".hg")
    cl_path = os.path.join(hg_path, "00changelog.i")
    if os.path.exists(os.path.join(cl_path)):
        return os.stat(cl_path).st_mtime
    else:
        return os.stat(hg_path).st_mtime

def staticfile(directory, fname):
    """return a file inside directory with guessed content-type header

    fname always uses '/' as directory separator and isn't allowed to
    contain unusual path components.
    Content-type is guessed using the mimetypes module.
    Return an empty string if fname is illegal or file not found.

    """
    parts = fname.split('/')
    path = directory
    for part in parts:
        if (part in ('', os.curdir, os.pardir) or
            os.sep in part or os.altsep is not None and os.altsep in part):
            return ""
        path = os.path.join(path, part)
    try:
        os.stat(path)
        ct = mimetypes.guess_type(path)[0] or "text/plain"
        return "Content-type: %s\n\n%s" % (ct, file(path).read())
    except (TypeError, OSError):
        # illegal fname or unreadable file
        return ""

class hgrequest(object):
    def __init__(self, inp=None, out=None, env=None):
        self.inp = inp or sys.stdin
        self.out = out or sys.stdout
        self.env = env or os.environ
        self.form = cgi.parse(self.inp, self.env, keep_blank_values=1)

    def write(self, *things):
        for thing in things:
            if hasattr(thing, "__iter__"):
                for part in thing:
                    self.write(part)
            else:
                try:
                    self.out.write(str(thing))
                except socket.error, inst:
                    if inst[0] != errno.ECONNRESET:
                        raise

    def header(self, headers=[('Content-type','text/html')]):
        for header in headers:
            self.out.write("%s: %s\r\n" % header)
        self.out.write("\r\n")

    def httphdr(self, type, file="", size=0):

        headers = [('Content-type', type)]
        if file:
            headers.append(('Content-disposition', 'attachment; filename=%s' % file))
        if size > 0:
            headers.append(('Content-length', str(size)))
        self.header(headers)

class hgweb(object):
    def __init__(self, repo, name=None):
        if type(repo) == type(""):
            self.repo = hg.repository(ui.ui(), repo)
        else:
            self.repo = repo

        self.mtime = -1
        self.reponame = name
        self.archives = 'zip', 'gz', 'bz2'

    def refresh(self):
        mtime = get_mtime(self.repo.root)
        if mtime != self.mtime:
            self.mtime = mtime
            self.repo = hg.repository(self.repo.ui, self.repo.root)
            self.maxchanges = int(self.repo.ui.config("web", "maxchanges", 10))
            self.maxfiles = int(self.repo.ui.config("web", "maxfiles", 10))
            self.allowpull = self.repo.ui.configbool("web", "allowpull", True)

    def archivelist(self, nodeid):
        for i in self.archives:
            if self.repo.ui.configbool("web", "allow" + i, False):
                yield {"type" : i, "node" : nodeid}

    def listfiles(self, files, mf):
        for f in files[:self.maxfiles]:
            yield self.t("filenodelink", node=hex(mf[f]), file=f)
        if len(files) > self.maxfiles:
            yield self.t("fileellipses")

    def listfilediffs(self, files, changeset):
        for f in files[:self.maxfiles]:
            yield self.t("filedifflink", node=hex(changeset), file=f)
        if len(files) > self.maxfiles:
            yield self.t("fileellipses")

    def siblings(self, siblings=[], rev=None, hiderev=None, **args):
        if not rev:
            rev = lambda x: ""
        siblings = [s for s in siblings if s != nullid]
        if len(siblings) == 1 and rev(siblings[0]) == hiderev:
            return
        for s in siblings:
            yield dict(node=hex(s), rev=rev(s), **args)

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

        modified, added, removed, deleted, unknown = r.changes(node1, node2)
        if files:
            modified, added, removed = map(lambda x: filterfiles(files, x),
                                           (modified, added, removed))

        diffopts = self.repo.ui.diffopts()
        showfunc = diffopts['showfunc']
        ignorews = diffopts['ignorews']
        for f in modified:
            to = r.file(f).read(mmap1[f])
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                            showfunc=showfunc, ignorews=ignorews), f, tn)
        for f in added:
            to = None
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                            showfunc=showfunc, ignorews=ignorews), f, tn)
        for f in removed:
            to = r.file(f).read(mmap1[f])
            tn = None
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f,
                            showfunc=showfunc, ignorews=ignorews), f, tn)

    def changelog(self, pos):
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
            for f in seq(1, self.maxchanges):
                if f < self.maxchanges or f <= last:
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
                n = cl.node(i)
                changes = cl.read(n)
                hn = hex(n)

                l.insert(0, {"parity": parity,
                             "author": changes[1],
                             "parent": self.siblings(cl.parents(n), cl.rev,
                                                     cl.rev(n) - 1),
                             "child": self.siblings(cl.children(n), cl.rev,
                                                    cl.rev(n) + 1),
                             "changelogtag": self.showtag("changelogtag",n),
                             "manifest": hex(changes[0]),
                             "desc": changes[4],
                             "date": changes[2],
                             "files": self.listfilediffs(changes[3], n),
                             "rev": i,
                             "node": hn})
                parity = 1 - parity

            for e in l:
                yield e

        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]
        count = cl.count()
        start = max(0, pos - self.maxchanges + 1)
        end = min(count, start + self.maxchanges)
        pos = end - 1

        yield self.t('changelog',
                     changenav=changenav,
                     manifest=hex(mf),
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
                        n = cl.node(j)
                        changes = cl.read(n)
                        l.append((n, j, changes))
                    l.reverse()
                    for e in l:
                        yield e

            for n, i, changes in revgen():
                miss = 0
                for q in qw:
                    if not (q in changes[1].lower() or
                            q in changes[4].lower() or
                            q in " ".join(changes[3][:20]).lower()):
                        miss = 1
                        break
                if miss:
                    continue

                count += 1
                hn = hex(n)

                yield self.t('searchentry',
                             parity=count & 1,
                             author=changes[1],
                             parent=self.siblings(cl.parents(n), cl.rev),
                             child=self.siblings(cl.children(n), cl.rev),
                             changelogtag=self.showtag("changelogtag",n),
                             manifest=hex(changes[0]),
                             desc=changes[4],
                             date=changes[2],
                             files=self.listfilediffs(changes[3], n),
                             rev=i,
                             node=hn)

                if count >= self.maxchanges:
                    break

        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

        yield self.t('search',
                     query=query,
                     manifest=hex(mf),
                     entries=changelist)

    def changeset(self, nodeid):
        cl = self.repo.changelog
        n = self.repo.lookup(nodeid)
        nodeid = hex(n)
        changes = cl.read(n)
        p1 = cl.parents(n)[0]

        files = []
        mf = self.repo.manifest.read(changes[0])
        for f in changes[3]:
            files.append(self.t("filenodelink",
                                filenode=hex(mf.get(f, nullid)), file=f))

        def diff(**map):
            yield self.diff(p1, n, None)

        yield self.t('changeset',
                     diff=diff,
                     rev=cl.rev(n),
                     node=nodeid,
                     parent=self.siblings(cl.parents(n), cl.rev),
                     child=self.siblings(cl.children(n), cl.rev),
                     changesettag=self.showtag("changesettag",n),
                     manifest=hex(changes[0]),
                     author=changes[1],
                     desc=changes[4],
                     date=changes[2],
                     files=files,
                     archives=self.archivelist(nodeid))

    def filelog(self, f, filenode):
        cl = self.repo.changelog
        fl = self.repo.file(f)
        filenode = hex(fl.lookup(filenode))
        count = fl.count()

        def entries(**map):
            l = []
            parity = (count - 1) & 1

            for i in range(count):
                n = fl.node(i)
                lr = fl.linkrev(n)
                cn = cl.node(lr)
                cs = cl.read(cl.node(lr))

                l.insert(0, {"parity": parity,
                             "filenode": hex(n),
                             "filerev": i,
                             "file": f,
                             "node": hex(cn),
                             "author": cs[1],
                             "date": cs[2],
                             "rename": self.renamelink(fl, n),
                             "parent": self.siblings(fl.parents(n),
                                                     fl.rev, file=f),
                             "child": self.siblings(fl.children(n),
                                                    fl.rev, file=f),
                             "desc": cs[4]})
                parity = 1 - parity

            for e in l:
                yield e

        yield self.t("filelog", file=f, filenode=filenode, entries=entries)

    def filerevision(self, f, node):
        fl = self.repo.file(f)
        n = fl.lookup(node)
        node = hex(n)
        text = fl.read(n)
        changerev = fl.linkrev(n)
        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        mfn = cs[0]

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
                       "parity": l & 1}

        yield self.t("filerevision",
                     file=f,
                     filenode=node,
                     path=up(f),
                     text=lines(),
                     raw=rawtext,
                     mimetype=mt,
                     rev=changerev,
                     node=hex(cn),
                     manifest=hex(mfn),
                     author=cs[1],
                     date=cs[2],
                     parent=self.siblings(fl.parents(n), fl.rev, file=f),
                     child=self.siblings(fl.children(n), fl.rev, file=f),
                     rename=self.renamelink(fl, n),
                     permissions=self.repo.manifest.readflags(mfn)[f])

    def fileannotate(self, f, node):
        bcache = {}
        ncache = {}
        fl = self.repo.file(f)
        n = fl.lookup(node)
        node = hex(n)
        changerev = fl.linkrev(n)

        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        mfn = cs[0]

        def annotate(**map):
            parity = 1
            last = None
            for r, l in fl.annotate(n):
                try:
                    cnode = ncache[r]
                except KeyError:
                    cnode = ncache[r] = self.repo.changelog.node(r)

                try:
                    name = bcache[r]
                except KeyError:
                    cl = self.repo.changelog.read(cnode)
                    bcache[r] = name = self.repo.ui.shortuser(cl[1])

                if last != cnode:
                    parity = 1 - parity
                    last = cnode

                yield {"parity": parity,
                       "node": hex(cnode),
                       "rev": r,
                       "author": name,
                       "file": f,
                       "line": l}

        yield self.t("fileannotate",
                     file=f,
                     filenode=node,
                     annotate=annotate,
                     path=up(f),
                     rev=changerev,
                     node=hex(cn),
                     manifest=hex(mfn),
                     author=cs[1],
                     date=cs[2],
                     rename=self.renamelink(fl, n),
                     parent=self.siblings(fl.parents(n), fl.rev, file=f),
                     child=self.siblings(fl.children(n), fl.rev, file=f),
                     permissions=self.repo.manifest.readflags(mfn)[f])

    def manifest(self, mnode, path):
        man = self.repo.manifest
        mn = man.lookup(mnode)
        mnode = hex(mn)
        mf = man.read(mn)
        rev = man.rev(mn)
        node = self.repo.changelog.node(rev)
        mff = man.readflags(mn)

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
                short = remain[:remain.find("/") + 1] # bleah
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
                       "manifest": mnode,
                       "filenode": hex(fnode),
                       "parity": parity,
                       "basename": f,
                       "permissions": mff[full]}
                parity = 1 - parity

        def dirlist(**map):
            parity = 0
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if fnode:
                    continue

                yield {"parity": parity,
                       "path": os.path.join(path, f),
                       "manifest": mnode,
                       "basename": f[:-1]}
                parity = 1 - parity

        yield self.t("manifest",
                     manifest=mnode,
                     rev=rev,
                     node=hex(node),
                     path=path,
                     up=up(path),
                     fentries=filelist,
                     dentries=dirlist,
                     archives=self.archivelist(hex(node)))

    def tags(self):
        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

        i = self.repo.tagslist()
        i.reverse()

        def entries(notip=False, **map):
            parity = 0
            for k,n in i:
                if notip and k == "tip": continue
                yield {"parity": parity,
                       "tag": k,
                       "tagmanifest": hex(cl.read(n)[0]),
                       "date": cl.read(n)[2],
                       "node": hex(n)}
                parity = 1 - parity

        yield self.t("tags",
                     manifest=hex(mf),
                     entries=lambda **x: entries(False, **x),
                     entriesnotip=lambda **x: entries(True, **x))

    def summary(self):
        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

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
                m = c[0]
                t = c[2]

                yield self.t("tagentry",
                             parity = parity,
                             tag = k,
                             node = hex(n),
                             date = t,
                             tagmanifest = hex(m))
                parity = 1 - parity

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
                    manifest = hex(changes[0]),
                    desc = changes[4],
                    date = t,
                    rev = i,
                    node = hn))
                parity = 1 - parity

            yield l

        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]
        count = cl.count()
        start = max(0, count - self.maxchanges)
        end = min(count, start + self.maxchanges)
        pos = end - 1

        yield self.t("summary",
                 desc = self.repo.ui.config("web", "description", "unknown"),
                 owner = (self.repo.ui.config("ui", "username") or # preferred
                          self.repo.ui.config("web", "contact") or # deprecated
                          self.repo.ui.config("web", "author", "unknown")), # also
                 lastchange = (0, 0), # FIXME
                 manifest = hex(mf),
                 tags = tagentries,
                 shortlog = changelist)

    def filediff(self, file, changeset):
        cl = self.repo.changelog
        n = self.repo.lookup(changeset)
        changeset = hex(n)
        p1 = cl.parents(n)[0]
        cs = cl.read(n)
        mf = self.repo.manifest.read(cs[0])

        def diff(**map):
            yield self.diff(p1, n, file)

        yield self.t("filediff",
                     file=file,
                     filenode=hex(mf.get(file, nullid)),
                     node=changeset,
                     rev=self.repo.changelog.rev(n),
                     parent=self.siblings(cl.parents(n), cl.rev),
                     child=self.siblings(cl.children(n), cl.rev),
                     diff=diff)

    archive_specs = {
        'bz2': ('application/x-tar', 'tbz2', '.tar.bz2', 'x-bzip2'),
        'gz': ('application/x-tar', 'tgz', '.tar.gz', 'x-gzip'),
        'zip': ('application/zip', 'zip', '.zip', None),
        }

    def archive(self, req, cnode, type):
        reponame = re.sub(r"\W+", "-", os.path.basename(self.reponame))
        name = "%s-%s" % (reponame, short(cnode))
        mimetype, artype, extension, encoding = self.archive_specs[type]
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

    def run(self, req=hgrequest()):
        def clean(path):
            p = util.normpath(path)
            if p[:2] == "..":
                raise "suspicious path"
            return p

        def header(**map):
            yield self.t("header", **map)

        def footer(**map):
            yield self.t("footer",
                         motd=self.repo.ui.config("web", "motd", ""),
                         **map)

        def expand_form(form):
            shortcuts = {
                'cl': [('cmd', ['changelog']), ('rev', None)],
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

        self.refresh()

        expand_form(req.form)

        t = self.repo.ui.config("web", "templates", templater.templatepath())
        static = self.repo.ui.config("web", "static", os.path.join(t,"static"))
        m = os.path.join(t, "map")
        style = self.repo.ui.config("web", "style", "")
        if req.form.has_key('style'):
            style = req.form['style'][0]
        if style:
            b = os.path.basename("map-" + style)
            p = os.path.join(t, b)
            if os.path.isfile(p):
                m = p

        port = req.env["SERVER_PORT"]
        port = port != "80" and (":" + port) or ""
        uri = req.env["REQUEST_URI"]
        if "?" in uri:
            uri = uri.split("?")[0]
        url = "http://%s%s%s" % (req.env["SERVER_NAME"], port, uri)
        if not self.reponame:
            self.reponame = (self.repo.ui.config("web", "name")
                             or uri.strip('/') or self.repo.root)

        self.t = templater.templater(m, templater.common_filters,
                                     defaults={"url": url,
                                               "repo": self.reponame,
                                               "header": header,
                                               "footer": footer,
                                               })

        if not req.form.has_key('cmd'):
            req.form['cmd'] = [self.t.cache['default'],]

        cmd = req.form['cmd'][0]
        if cmd == 'changelog':
            hi = self.repo.changelog.count() - 1
            if req.form.has_key('rev'):
                hi = req.form['rev'][0]
                try:
                    hi = self.repo.changelog.rev(self.repo.lookup(hi))
                except hg.RepoError:
                    req.write(self.search(hi)) # XXX redirect to 404 page?
                    return

            req.write(self.changelog(hi))

        elif cmd == 'changeset':
            req.write(self.changeset(req.form['node'][0]))

        elif cmd == 'manifest':
            req.write(self.manifest(req.form['manifest'][0],
                                    clean(req.form['path'][0])))

        elif cmd == 'tags':
            req.write(self.tags())

        elif cmd == 'summary':
            req.write(self.summary())

        elif cmd == 'filediff':
            req.write(self.filediff(clean(req.form['file'][0]),
                                    req.form['node'][0]))

        elif cmd == 'file':
            req.write(self.filerevision(clean(req.form['file'][0]),
                                        req.form['filenode'][0]))

        elif cmd == 'annotate':
            req.write(self.fileannotate(clean(req.form['file'][0]),
                                        req.form['filenode'][0]))

        elif cmd == 'filelog':
            req.write(self.filelog(clean(req.form['file'][0]),
                                   req.form['filenode'][0]))

        elif cmd == 'heads':
            req.httphdr("application/mercurial-0.1")
            h = self.repo.heads()
            req.write(" ".join(map(hex, h)) + "\n")

        elif cmd == 'branches':
            req.httphdr("application/mercurial-0.1")
            nodes = []
            if req.form.has_key('nodes'):
                nodes = map(bin, req.form['nodes'][0].split(" "))
            for b in self.repo.branches(nodes):
                req.write(" ".join(map(hex, b)) + "\n")

        elif cmd == 'between':
            req.httphdr("application/mercurial-0.1")
            nodes = []
            if req.form.has_key('pairs'):
                pairs = [map(bin, p.split("-"))
                         for p in req.form['pairs'][0].split(" ")]
            for b in self.repo.between(pairs):
                req.write(" ".join(map(hex, b)) + "\n")

        elif cmd == 'changegroup':
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

        elif cmd == 'archive':
            changeset = self.repo.lookup(req.form['node'][0])
            type = req.form['type'][0]
            if (type in self.archives and
                self.repo.ui.configbool("web", "allow" + type, False)):
                self.archive(req, changeset, type)
                return

            req.write(self.t("error"))

        elif cmd == 'static':
            fname = req.form['file'][0]
            req.write(staticfile(static, fname)
                      or self.t("error", error="%r not found" % fname))

        else:
            req.write(self.t("error"))

def create_server(ui, repo):
    use_threads = True

    def openlog(opt, default):
        if opt and opt != '-':
            return open(opt, 'w')
        return default

    address = ui.config("web", "address", "")
    port = int(ui.config("web", "port", 8000))
    use_ipv6 = ui.configbool("web", "ipv6")
    webdir_conf = ui.config("web", "webdir_conf")
    accesslog = openlog(ui.config("web", "accesslog", "-"), sys.stdout)
    errorlog = openlog(ui.config("web", "errorlog", "-"), sys.stderr)

    if use_threads:
        try:
            from threading import activeCount
        except ImportError:
            use_threads = False

    if use_threads:
        _mixin = SocketServer.ThreadingMixIn
    else:
        if hasattr(os, "fork"):
            _mixin = SocketServer.ForkingMixIn
        else:
            class _mixin: pass

    class MercurialHTTPServer(_mixin, BaseHTTPServer.HTTPServer):
        pass

    class IPv6HTTPServer(MercurialHTTPServer):
        address_family = getattr(socket, 'AF_INET6', None)

        def __init__(self, *args, **kwargs):
            if self.address_family is None:
                raise hg.RepoError(_('IPv6 not available on this system'))
            BaseHTTPServer.HTTPServer.__init__(self, *args, **kwargs)

    class hgwebhandler(BaseHTTPServer.BaseHTTPRequestHandler):

        def log_error(self, format, *args):
            errorlog.write("%s - - [%s] %s\n" % (self.address_string(),
                                                 self.log_date_time_string(),
                                                 format % args))

        def log_message(self, format, *args):
            accesslog.write("%s - - [%s] %s\n" % (self.address_string(),
                                                  self.log_date_time_string(),
                                                  format % args))

        def do_POST(self):
            try:
                self.do_hgweb()
            except socket.error, inst:
                if inst[0] != errno.EPIPE:
                    raise

        def do_GET(self):
            self.do_POST()

        def do_hgweb(self):
            path_info, query = splitURI(self.path)

            env = {}
            env['GATEWAY_INTERFACE'] = 'CGI/1.1'
            env['REQUEST_METHOD'] = self.command
            env['SERVER_NAME'] = self.server.server_name
            env['SERVER_PORT'] = str(self.server.server_port)
            env['REQUEST_URI'] = "/"
            env['PATH_INFO'] = path_info
            if query:
                env['QUERY_STRING'] = query
            host = self.address_string()
            if host != self.client_address[0]:
                env['REMOTE_HOST'] = host
                env['REMOTE_ADDR'] = self.client_address[0]

            if self.headers.typeheader is None:
                env['CONTENT_TYPE'] = self.headers.type
            else:
                env['CONTENT_TYPE'] = self.headers.typeheader
            length = self.headers.getheader('content-length')
            if length:
                env['CONTENT_LENGTH'] = length
            accept = []
            for line in self.headers.getallmatchingheaders('accept'):
                if line[:1] in "\t\n\r ":
                    accept.append(line.strip())
                else:
                    accept = accept + line[7:].split(',')
            env['HTTP_ACCEPT'] = ','.join(accept)

            req = hgrequest(self.rfile, self.wfile, env)
            self.send_response(200, "Script output follows")

            if webdir_conf:
                hgwebobj = hgwebdir(webdir_conf)
            elif repo is not None:
                hgwebobj = hgweb(repo.__class__(repo.ui, repo.origroot))
            else:
                raise hg.RepoError(_('no repo found'))
            hgwebobj.run(req)


    if use_ipv6:
        return IPv6HTTPServer((address, port), hgwebhandler)
    else:
        return MercurialHTTPServer((address, port), hgwebhandler)

# This is a stopgap
class hgwebdir(object):
    def __init__(self, config):
        def cleannames(items):
            return [(name.strip(os.sep), path) for name, path in items]

        self.motd = ""
        if isinstance(config, (list, tuple)):
            self.repos = cleannames(config)
        elif isinstance(config, dict):
            self.repos = cleannames(config.items())
            self.repos.sort()
        else:
            cp = ConfigParser.SafeConfigParser()
            cp.read(config)
            self.repos = []
            if cp.has_section('web') and cp.has_option('web', 'motd'):
                self.motd = cp.get('web', 'motd')
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

    def run(self, req=hgrequest()):
        def header(**map):
            yield tmpl("header", **map)

        def footer(**map):
            yield tmpl("footer", motd=self.motd, **map)

        m = os.path.join(templater.templatepath(), "map")
        tmpl = templater.templater(m, templater.common_filters,
                                   defaults={"header": header,
                                             "footer": footer})

        def entries(**map):
            parity = 0
            for name, path in self.repos:
                u = ui.ui()
                try:
                    u.readconfig(os.path.join(path, '.hg', 'hgrc'))
                except IOError:
                    pass
                get = u.config

                url = ('/'.join([req.env["REQUEST_URI"].split('?')[0], name])
                       .replace("//", "/"))

                # update time with local timezone
                try:
                    d = (get_mtime(path), util.makedate()[1])
                except OSError:
                    continue

                yield dict(contact=(get("ui", "username") or # preferred
                                    get("web", "contact") or # deprecated
                                    get("web", "author", "unknown")), # also
                           name=get("web", "name", name),
                           url=url,
                           parity=parity,
                           shortdesc=get("web", "description", "unknown"),
                           lastupdate=d)

                parity = 1 - parity

        virtual = req.env.get("PATH_INFO", "").strip('/')
        if virtual:
            real = dict(self.repos).get(virtual)
            if real:
                try:
                    hgweb(real).run(req)
                except IOError, inst:
                    req.write(tmpl("error", error=inst.strerror))
                except hg.RepoError, inst:
                    req.write(tmpl("error", error=str(inst)))
            else:
                req.write(tmpl("notfound", repo=virtual))
        else:
            if req.form.has_key('static'):
                static = os.path.join(templater.templatepath(), "static")
                fname = req.form['static'][0]
                req.write(staticfile(static, fname)
                          or tmpl("error", error="%r not found" % fname))
            else:
                req.write(tmpl("index", entries=entries))
