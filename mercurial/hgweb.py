# hgweb.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, cgi, time, re, socket, sys, zlib
import mdiff
from hg import *
from ui import *


def templatepath():
    for f in "templates", "../templates":
        p = os.path.join(os.path.dirname(__file__), f)
        if os.path.isdir(p):
            return p

def age(t):
    def plural(t, c):
        if c == 1:
            return t
        return t + "s"
    def fmt(t, c):
        return "%d %s" % (c, plural(t, c))

    now = time.time()
    delta = max(1, int(now - t))

    scales = [["second", 1],
              ["minute", 60],
              ["hour", 3600],
              ["day", 3600 * 24],
              ["week", 3600 * 24 * 7],
              ["month", 3600 * 24 * 30],
              ["year", 3600 * 24 * 365]]

    scales.reverse()

    for t, s in scales:
        n = delta / s
        if n >= 2 or s == 1:
            return fmt(t, n)

def nl2br(text):
    return text.replace('\n', '<br/>\n')

def obfuscate(text):
    return ''.join(['&#%d;' % ord(c) for c in text])

def up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def httphdr(type, file="", size=0):
    sys.stdout.write('Content-type: %s\n' % type)
    if file:
        sys.stdout.write('Content-disposition: attachment; filename=%s\n'
            % file)
    if size > 0:
        sys.stdout.write('Content-length: %d\n' % size)
    sys.stdout.write('\n')

def write(*things):
    for thing in things:
        if hasattr(thing, "__iter__"):
            for part in thing:
                write(part)
        else:
            try:
                sys.stdout.write(str(thing))
            except socket.error, x:
                if x[0] != errno.ECONNRESET:
                    raise

class templater:
    def __init__(self, mapfile, filters={}, defaults={}):
        self.cache = {}
        self.map = {}
        self.base = os.path.dirname(mapfile)
        self.filters = filters
        self.defaults = defaults

        for l in file(mapfile):
            m = re.match(r'(\S+)\s*=\s*"(.*)"$', l)
            if m:
                self.cache[m.group(1)] = m.group(2)
            else:
                m = re.match(r'(\S+)\s*=\s*(\S+)', l)
                if m:
                    self.map[m.group(1)] = os.path.join(self.base, m.group(2))
                else:
                    raise LookupError("unknown map entry '%s'" % l)

    def __call__(self, t, **map):
        m = self.defaults.copy()
        m.update(map)
        try:
            tmpl = self.cache[t]
        except KeyError:
            tmpl = self.cache[t] = file(self.map[t]).read()
        return self.template(tmpl, self.filters, **m)

    def template(self, tmpl, filters={}, **map):
        while tmpl:
            m = re.search(r"#([a-zA-Z0-9]+)((%[a-zA-Z0-9]+)*)((\|[a-zA-Z0-9]+)*)#", tmpl)
            if m:
                yield tmpl[:m.start(0)]
                v = map.get(m.group(1), "")
                v = callable(v) and v(**map) or v

                format = m.group(2)
                fl = m.group(4)

                if format:
                    q = v.__iter__
                    for i in q():
                        lm = map.copy()
                        lm.update(i)
                        yield self(format[1:], **lm)

                    v = ""

                elif fl:
                    for f in fl.split("|")[1:]:
                        v = filters[f](v)

                yield v
                tmpl = tmpl[m.end(0):]
            else:
                yield tmpl
                return

def rfc822date(x):
    return time.strftime("%a, %d %b %Y %H:%M:%S +0000", time.gmtime(x))

common_filters = {
    "escape": cgi.escape,
    "age": age,
    "date": (lambda x: time.asctime(time.gmtime(x))),
    "addbreaks": nl2br,
    "obfuscate": obfuscate,
    "short": (lambda x: x[:12]),
    "firstline": (lambda x: x.splitlines(1)[0]),
    "permissions": (lambda x: x and "-rwxr-xr-x" or "-rw-r--r--"),
    "rfc822date": rfc822date,
    }

class hgweb:
    def __init__(self, repo, name=None):
        if type(repo) == type(""):
            self.repo = repository(ui(), repo)
        else:
            self.repo = repo

        self.mtime = -1
        self.reponame = name or self.repo.ui.config("web", "name",
                                                    self.repo.root)
        self.archives = 'zip', 'gz', 'bz2'

    def refresh(self):
        s = os.stat(os.path.join(self.repo.root, ".hg", "00changelog.i"))
        if s.st_mtime != self.mtime:
            self.mtime = s.st_mtime
            self.repo = repository(self.repo.ui, self.repo.root)
            self.maxchanges = self.repo.ui.config("web", "maxchanges", 10)
            self.maxfiles = self.repo.ui.config("web", "maxchanges", 10)
            self.allowpull = self.repo.ui.configbool("web", "allowpull", True)

    def date(self, cs):
        return time.asctime(time.gmtime(float(cs[2].split(' ')[0])))

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

    def parents(self, t1, nodes=[], rev=None,**args):
        if not rev:
            rev = lambda x: ""
        for node in nodes:
            if node != nullid:
                yield self.t(t1, node=hex(node), rev=rev(node), **args)

    def showtag(self, t1, node=nullid, **args):
        for t in self.repo.nodetags(node):
             yield self.t(t1, tag=t, **args)

    def diff(self, node1, node2, files):
        def filterfiles(list, files):
            l = [x for x in list if x in files]

            for f in files:
                if f[-1] != os.sep:
                    f += os.sep
                l += [x for x in list if x.startswith(f)]
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
        date1 = self.date(change1)
        date2 = self.date(change2)

        c, a, d, u = r.changes(node1, node2)
        if files:
            c, a, d = map(lambda x: filterfiles(x, files), (c, a, d))

        for f in c:
            to = r.file(f).read(mmap1[f])
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f), f, tn)
        for f in a:
            to = None
            tn = r.file(f).read(mmap2[f])
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f), f, tn)
        for f in d:
            to = r.file(f).read(mmap1[f])
            tn = None
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f), f, tn)

    def changelog(self, pos):
        def changenav(**map):
            def seq(factor=1):
                yield 1 * factor
                yield 3 * factor
                #yield 5 * factor
                for f in seq(factor * 10):
                    yield f

            l = []
            for f in seq():
                if f < self.maxchanges / 2:
                    continue
                if f > count:
                    break
                r = "%d" % f
                if pos + f < count:
                    l.append(("+" + r, pos + f))
                if pos - f >= 0:
                    l.insert(0, ("-" + r, pos - f))

            yield {"rev": 0, "label": "(0)"}

            for label, rev in l:
                yield {"label": label, "rev": rev}

            yield {"label": "tip", "rev": ""}

        def changelist(**map):
            parity = (start - end) & 1
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in range(start, end):
                n = cl.node(i)
                changes = cl.read(n)
                hn = hex(n)
                t = float(changes[2].split(' ')[0])

                l.insert(0, {"parity": parity,
                             "author": changes[1],
                             "parent": self.parents("changelogparent",
                                                    cl.parents(n), cl.rev),
                             "changelogtag": self.showtag("changelogtag",n),
                             "manifest": hex(changes[0]),
                             "desc": changes[4],
                             "date": t,
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
                     rev=pos, changesets=count, entries=changelist)

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
                t = float(changes[2].split(' ')[0])

                yield self.t('searchentry',
                             parity=count & 1,
                             author=changes[1],
                             parent=self.parents("changelogparent",
                                                 cl.parents(n), cl.rev),
                             changelogtag=self.showtag("changelogtag",n),
                             manifest=hex(changes[0]),
                             desc=changes[4],
                             date=t,
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
        n = bin(nodeid)
        cl = self.repo.changelog
        changes = cl.read(n)
        p1 = cl.parents(n)[0]
        t = float(changes[2].split(' ')[0])

        files = []
        mf = self.repo.manifest.read(changes[0])
        for f in changes[3]:
            files.append(self.t("filenodelink",
                                filenode=hex(mf.get(f, nullid)), file=f))

        def diff(**map):
            yield self.diff(p1, n, None)

        def archivelist():
            for i in self.archives:
                if self.repo.ui.configbool("web", "allow" + i, False):
                    yield {"type" : i, "node" : nodeid}

        yield self.t('changeset',
                     diff=diff,
                     rev=cl.rev(n),
                     node=nodeid,
                     parent=self.parents("changesetparent",
                                         cl.parents(n), cl.rev),
                     changesettag=self.showtag("changesettag",n),
                     manifest=hex(changes[0]),
                     author=changes[1],
                     desc=changes[4],
                     date=t,
                     files=files,
                     archives=archivelist())

    def filelog(self, f, filenode):
        cl = self.repo.changelog
        fl = self.repo.file(f)
        count = fl.count()

        def entries(**map):
            l = []
            parity = (count - 1) & 1

            for i in range(count):
                n = fl.node(i)
                lr = fl.linkrev(n)
                cn = cl.node(lr)
                cs = cl.read(cl.node(lr))
                t = float(cs[2].split(' ')[0])

                l.insert(0, {"parity": parity,
                             "filenode": hex(n),
                             "filerev": i,
                             "file": f,
                             "node": hex(cn),
                             "author": cs[1],
                             "date": t,
                             "parent": self.parents("filelogparent",
                                                    fl.parents(n),
                                                    fl.rev, file=f),
                             "desc": cs[4]})
                parity = 1 - parity

            for e in l:
                yield e

        yield self.t("filelog", file=f, filenode=filenode, entries=entries)

    def filerevision(self, f, node):
        fl = self.repo.file(f)
        n = bin(node)
        text = fl.read(n)
        changerev = fl.linkrev(n)
        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        t = float(cs[2].split(' ')[0])
        mfn = cs[0]

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
                     rev=changerev,
                     node=hex(cn),
                     manifest=hex(mfn),
                     author=cs[1],
                     date=t,
                     parent=self.parents("filerevparent",
                                         fl.parents(n), fl.rev, file=f),
                     permissions=self.repo.manifest.readflags(mfn)[f])

    def fileannotate(self, f, node):
        bcache = {}
        ncache = {}
        fl = self.repo.file(f)
        n = bin(node)
        changerev = fl.linkrev(n)

        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        t = float(cs[2].split(' ')[0])
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
                    name = cl[1]
                    f = name.find('@')
                    if f >= 0:
                        name = name[:f]
                    f = name.find('<')
                    if f >= 0:
                        name = name[f+1:]
                    bcache[r] = name

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
                     date=t,
                     parent=self.parents("fileannotateparent",
                                         fl.parents(n), fl.rev, file=f),
                     permissions=self.repo.manifest.readflags(mfn)[f])

    def manifest(self, mnode, path):
        mf = self.repo.manifest.read(bin(mnode))
        rev = self.repo.manifest.rev(bin(mnode))
        node = self.repo.changelog.node(rev)
        mff=self.repo.manifest.readflags(bin(mnode))

        files = {}

        p = path[1:]
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
                     dentries=dirlist)

    def tags(self):
        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

        i = self.repo.tagslist()
        i.reverse()

        def entries(**map):
            parity = 0
            for k,n in i:
                yield {"parity": parity,
                       "tag": k,
                       "node": hex(n)}
                parity = 1 - parity

        yield self.t("tags",
                     manifest=hex(mf),
                     entries=entries)

    def filediff(self, file, changeset):
        n = bin(changeset)
        cl = self.repo.changelog
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
                     parent=self.parents("filediffparent",
                                         cl.parents(n), cl.rev),
                     diff=diff)

    def archive(self, cnode, type):
        cs = self.repo.changelog.read(cnode)
        mnode = cs[0]
        mf = self.repo.manifest.read(mnode)
        rev = self.repo.manifest.rev(mnode)
        reponame = re.sub(r"\W+", "-", self.reponame)
        name = "%s-%s/" % (reponame, short(cnode))

        files = mf.keys()
        files.sort()

        if type == 'zip':
            import zipfile

            try:
                tmp = tempfile.mkstemp()[1]
                zf = zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED)

                for f in files:
                    zf.writestr(name + f, self.repo.file(f).read(mf[f]))
                zf.close()

                f = open(tmp, 'r')
                httphdr('application/zip', name[:-1] + '.zip',
                        os.path.getsize(tmp))
                sys.stdout.write(f.read())
                f.close()
            finally:
                os.unlink(tmp)

        else:
            import StringIO
            import time
            import tarfile

            tf = tarfile.TarFile.open(mode='w|' + type, fileobj=sys.stdout)
            mff = self.repo.manifest.readflags(mnode)
            mtime = int(time.time())

            httphdr('application/octet-stream', name[:-1] + '.tar.' + type)
            for fname in files:
                rcont = self.repo.file(fname).read(mf[fname])
                finfo = tarfile.TarInfo(name + fname)
                finfo.mtime = mtime
                finfo.size = len(rcont)
                finfo.mode = mff[fname] and 0755 or 0644
                tf.addfile(finfo, StringIO.StringIO(rcont))
            tf.close()

    # add tags to things
    # tags -> list of changesets corresponding to tags
    # find tag, changeset, file

    def run(self):
        def header(**map):
            yield self.t("header", **map)

        def footer(**map):
            yield self.t("footer", **map)

        self.refresh()
        args = cgi.parse()

        t = self.repo.ui.config("web", "templates", templatepath())
        m = os.path.join(t, "map")
        style = self.repo.ui.config("web", "style", "")
        if args.has_key('style'):
            style = args['style'][0]
        if style:
            b = os.path.basename("map-" + style)
            p = os.path.join(t, b)
            if os.path.isfile(p):
                m = p

        port = os.environ["SERVER_PORT"]
        port = port != "80" and (":" + port) or ""
        uri = os.environ["REQUEST_URI"]
        if "?" in uri:
            uri = uri.split("?")[0]
        url = "http://%s%s%s" % (os.environ["SERVER_NAME"], port, uri)

        self.t = templater(m, common_filters,
                           {"url": url,
                            "repo": self.reponame,
                            "header": header,
                            "footer": footer,
                           })

        if not args.has_key('cmd'):
            args['cmd'] = [self.t.cache['default'],]

        if args['cmd'][0] == 'changelog':
            c = self.repo.changelog.count() - 1
            hi = c
            if args.has_key('rev'):
                hi = args['rev'][0]
                try:
                    hi = self.repo.changelog.rev(self.repo.lookup(hi))
                except RepoError:
                    write(self.search(hi))
                    return

            write(self.changelog(hi))

        elif args['cmd'][0] == 'changeset':
            write(self.changeset(args['node'][0]))

        elif args['cmd'][0] == 'manifest':
            write(self.manifest(args['manifest'][0], args['path'][0]))

        elif args['cmd'][0] == 'tags':
            write(self.tags())

        elif args['cmd'][0] == 'filediff':
            write(self.filediff(args['file'][0], args['node'][0]))

        elif args['cmd'][0] == 'file':
            write(self.filerevision(args['file'][0], args['filenode'][0]))

        elif args['cmd'][0] == 'annotate':
            write(self.fileannotate(args['file'][0], args['filenode'][0]))

        elif args['cmd'][0] == 'filelog':
            write(self.filelog(args['file'][0], args['filenode'][0]))

        elif args['cmd'][0] == 'heads':
            httphdr("application/mercurial-0.1")
            h = self.repo.heads()
            sys.stdout.write(" ".join(map(hex, h)) + "\n")

        elif args['cmd'][0] == 'branches':
            httphdr("application/mercurial-0.1")
            nodes = []
            if args.has_key('nodes'):
                nodes = map(bin, args['nodes'][0].split(" "))
            for b in self.repo.branches(nodes):
                sys.stdout.write(" ".join(map(hex, b)) + "\n")

        elif args['cmd'][0] == 'between':
            httphdr("application/mercurial-0.1")
            nodes = []
            if args.has_key('pairs'):
                pairs = [map(bin, p.split("-"))
                         for p in args['pairs'][0].split(" ")]
            for b in self.repo.between(pairs):
                sys.stdout.write(" ".join(map(hex, b)) + "\n")

        elif args['cmd'][0] == 'changegroup':
            httphdr("application/mercurial-0.1")
            nodes = []
            if not self.allowpull:
                return

            if args.has_key('roots'):
                nodes = map(bin, args['roots'][0].split(" "))

            z = zlib.compressobj()
            f = self.repo.changegroup(nodes)
            while 1:
                chunk = f.read(4096)
                if not chunk:
                    break
                sys.stdout.write(z.compress(chunk))

            sys.stdout.write(z.flush())

        elif args['cmd'][0] == 'archive':
            changeset = bin(args['node'][0])
            type = args['type'][0]
            if (type in self.archives and
                self.repo.ui.configbool("web", "allow" + type, False)):
                self.archive(changeset, type)
                return

            write(self.t("error"))

        else:
            write(self.t("error"))

def create_server(repo):

    def openlog(opt, default):
        if opt and opt != '-':
            return open(opt, 'w')
        return default

    address = repo.ui.config("web", "address", "")
    port = int(repo.ui.config("web", "port", 8000))
    use_ipv6 = repo.ui.configbool("web", "ipv6")
    accesslog = openlog(repo.ui.config("web", "accesslog", "-"), sys.stdout)
    errorlog = openlog(repo.ui.config("web", "errorlog", "-"), sys.stderr)

    import BaseHTTPServer

    class IPv6HTTPServer(BaseHTTPServer.HTTPServer):
        address_family = getattr(socket, 'AF_INET6', None)

        def __init__(self, *args, **kwargs):
            if self.address_family is None:
                raise RepoError('IPv6 not available on this system')
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
                if inst.args[0] != 32:
                    raise

        def do_GET(self):
            self.do_POST()

        def do_hgweb(self):
            query = ""
            p = self.path.find("?")
            if p:
                query = self.path[p + 1:]
                query = query.replace('+', ' ')

            env = {}
            env['GATEWAY_INTERFACE'] = 'CGI/1.1'
            env['REQUEST_METHOD'] = self.command
            env['SERVER_NAME'] = self.server.server_name
            env['SERVER_PORT'] = str(self.server.server_port)
            env['REQUEST_URI'] = "/"
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

            os.environ.update(env)

            save = sys.argv, sys.stdin, sys.stdout, sys.stderr
            try:
                sys.stdin = self.rfile
                sys.stdout = self.wfile
                sys.argv = ["hgweb.py"]
                if '=' not in query:
                    sys.argv.append(query)
                self.send_response(200, "Script output follows")
                hg.run()
            finally:
                sys.argv, sys.stdin, sys.stdout, sys.stderr = save

    hg = hgweb(repo)
    if use_ipv6:
        return IPv6HTTPServer((address, port), hgwebhandler)
    else:
        return BaseHTTPServer.HTTPServer((address, port), hgwebhandler)

def server(path, name, templates, address, port, use_ipv6=False,
           accesslog=sys.stdout, errorlog=sys.stderr):
    httpd = create_server(path, name, templates, address, port, use_ipv6,
                          accesslog, errorlog)
    httpd.serve_forever()

# This is a stopgap
class hgwebdir:
    def __init__(self, config):
        self.cp = ConfigParser.SafeConfigParser()
        self.cp.read(config)

    def run(self):
        try:
            virtual = os.environ["PATH_INFO"]
        except:
            virtual = ""

        if virtual[1:]:
            if self.cp.has_option("paths", virtual[1:]):
                real = self.cp.get("paths", virtual[1:])
                h = hgweb(real)
                h.run()
                return

        def header(**map):
            yield tmpl("header", **map)

        def footer(**map):
            yield tmpl("footer", **map)

        templates = templatepath()
        m = os.path.join(templates, "map")
        tmpl = templater(m, common_filters,
                         {"header": header, "footer": footer})

        def entries(**map):
            parity = 0
            l = self.cp.items("paths")
            l.sort()
            for v,r in l:
                cp2 = ConfigParser.SafeConfigParser()
                cp2.read(os.path.join(r, ".hg", "hgrc"))

                def get(sec, val, default):
                    try:
                        return cp2.get(sec, val)
                    except:
                        return default

                url = os.environ["REQUEST_URI"] + "/" + v
                url = url.replace("//", "/")

                yield dict(author=get("web", "author", "unknown"),
                           name=get("web", "name", v),
                           url=url,
                           parity=parity,
                           shortdesc=get("web", "description", "unknown"),
                           lastupdate=os.stat(os.path.join(r, ".hg",
                                                "00changelog.d")).st_mtime)

                parity = 1 - parity

        write(tmpl("index", entries=entries))
