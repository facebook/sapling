# hgweb.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com> 
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, cgi, time, re, difflib, sys, zlib
from mercurial.hg import *
from mercurial.ui import *

def templatepath():
    for f in "templates", "../templates":
        p = os.path.join(os.path.dirname(__file__), f)
        if os.path.isdir(p): return p

def age(t):
    def plural(t, c):
        if c == 1: return t
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
        if n >= 2 or s == 1: return fmt(t, n)

def nl2br(text):
    return text.replace('\n', '<br/>\n')

def obfuscate(text):
    return ''.join([ '&#%d;' % ord(c) for c in text ])

def up(p):
    if p[0] != "/": p = "/" + p
    if p[-1] == "/": p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def httphdr(type):
    print 'Content-type: %s\n' % type

def write(*things):
    for thing in things:
        if hasattr(thing, "__iter__"):
            for part in thing:
                write(part)
        else:
            sys.stdout.write(str(thing))

def template(tmpl, filters = {}, **map):
    while tmpl:
        m = re.search(r"#([a-zA-Z0-9]+)((\|[a-zA-Z0-9]+)*)#", tmpl)
        if m:
            yield tmpl[:m.start(0)]
            v = map.get(m.group(1), "")
            v = callable(v) and v() or v

            fl = m.group(2)
            if fl:
                for f in fl.split("|")[1:]:
                    v = filters[f](v)

            yield v
            tmpl = tmpl[m.end(0):]
        else:
            yield tmpl
            return

class templater:
    def __init__(self, mapfile, filters = {}):
        self.cache = {}
        self.map = {}
        self.base = os.path.dirname(mapfile)
        self.filters = filters

        for l in file(mapfile):
            m = re.match(r'(\S+)\s*=\s*"(.*)"$', l)
            if m:
                self.cache[m.group(1)] = m.group(2)
            else:
                m = re.match(r'(\S+)\s*=\s*(\S+)', l)
                if m:
                    self.map[m.group(1)] = os.path.join(self.base, m.group(2))
                else:
                    raise "unknown map entry '%s'"  % l

    def __call__(self, t, **map):
        try:
            tmpl = self.cache[t]
        except KeyError:
            tmpl = self.cache[t] = file(self.map[t]).read()
        return template(tmpl, self.filters, **map)

class hgweb:
    maxchanges = 10
    maxfiles = 10

    def __init__(self, path, name, templates = ""):
        self.templates = templates or templatepath()
        self.reponame = name
        self.path = path
        self.mtime = -1
        self.viewonly = 0

        self.filters = {
            "escape": cgi.escape,
            "age": age,
            "date": (lambda x: time.asctime(time.gmtime(x))),
            "addbreaks": nl2br,
            "obfuscate": obfuscate,
            "short": (lambda x: x[:12]),
            "firstline": (lambda x: x.splitlines(1)[0]),
            "permissions": (lambda x: x and "-rwxr-xr-x" or "-rw-r--r--")
            }

    def refresh(self):
        s = os.stat(os.path.join(self.path, ".hg", "00changelog.i"))
        if s.st_mtime != self.mtime:
            self.mtime = s.st_mtime
            self.repo = repository(ui(), self.path)

    def date(self, cs):
        return time.asctime(time.gmtime(float(cs[2].split(' ')[0])))

    def listfiles(self, files, mf):
        for f in files[:self.maxfiles]:
            yield self.t("filenodelink", node = hex(mf[f]), file = f)
        if len(files) > self.maxfiles:
            yield self.t("fileellipses")

    def listfilediffs(self, files, changeset):
        for f in files[:self.maxfiles]:
            yield self.t("filedifflink", node = hex(changeset), file = f)
        if len(files) > self.maxfiles:
            yield self.t("fileellipses")

    def parents(self, t1, nodes=[], rev=None,**args):
        if not rev: rev = lambda x: ""
        for node in nodes:
            if node != nullid:
                yield self.t(t1, node = hex(node), rev = rev(node), **args)

    def showtag(self, t1, node=nullid, **args):
        for t in self.repo.nodetags(node):
             yield self.t(t1, tag = t, **args)

    def diff(self, node1, node2, files):
        def filterfiles(list, files):
            l = [ x for x in list if x in files ]

            for f in files:
                if f[-1] != os.sep: f += os.sep
                l += [ x for x in list if x.startswith(f) ]
            return l

        parity = [0]
        def diffblock(diff, f, fn):
            yield self.t("diffblock",
                         lines = prettyprintlines(diff),
                         parity = parity[0],
                         file = f,
                         filenode = hex(fn or nullid))
            parity[0] = 1 - parity[0]

        def prettyprintlines(diff):
            for l in diff.splitlines(1):
                if l.startswith('+'):
                    yield self.t("difflineplus", line = l)
                elif l.startswith('-'):
                    yield self.t("difflineminus", line = l)
                elif l.startswith('@'):
                    yield self.t("difflineat", line = l)
                else:
                    yield self.t("diffline", line = l)

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

    def header(self):
        yield self.t("header", repo = self.reponame)

    def footer(self):
        yield self.t("footer", repo = self.reponame)

    def changelog(self, pos):
        def changenav():
            def seq(factor = 1):
                yield 1 * factor
                yield 3 * factor
                #yield 5 * factor
                for f in seq(factor * 10):
                    yield f

            l = []
            for f in seq():
                if f < self.maxchanges / 2: continue
                if f > count: break
                r = "%d" % f
                if pos + f < count: l.append(("+" + r, pos + f))
                if pos - f >= 0: l.insert(0, ("-" + r, pos - f))

            yield self.t("naventry", rev = 0, label="(0)")

            for label, rev in l:
                yield self.t("naventry", label = label, rev = rev)

            yield self.t("naventry", label="tip")

        def changelist():
            parity = (start - end) & 1
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in range(start, end):
                n = cl.node(i)
                changes = cl.read(n)
                hn = hex(n)
                p1, p2 = cl.parents(n)
                t = float(changes[2].split(' ')[0])

                l.insert(0, self.t(
                    'changelogentry',
                    parity = parity,
                    author = changes[1],
                    parent = self.parents("changelogparent",
                                          cl.parents(n), cl.rev),
                    changelogtag = self.showtag("changelogtag",n),
                    p1 = hex(p1), p2 = hex(p2),
                    p1rev = cl.rev(p1), p2rev = cl.rev(p2),
                    manifest = hex(changes[0]),
                    desc = changes[4],
                    date = t,
                    files = self.listfilediffs(changes[3], n),
                    rev = i,
                    node = hn))
                parity = 1 - parity

            yield l

        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]
        count = cl.count()
        start = max(0, pos - self.maxchanges + 1)
        end = min(count, start + self.maxchanges)
        pos = end - 1

        yield self.t('changelog',
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     changenav = changenav,
                     manifest = hex(mf),
                     rev = pos, changesets = count, entries = changelist)

    def search(self, query):

        def changelist():
            cl = self.repo.changelog
            count = 0
            qw = query.lower().split()

            def revgen():
                for i in range(cl.count() - 1, 0, -100):
                    l = []
                    for j in range(max(0, i - 100), i):
                        n = cl.node(j)
                        changes = cl.read(n)
                        l.insert(0, (n, j, changes))
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
                if miss: continue

                count += 1
                hn = hex(n)
                p1, p2 = cl.parents(n)
                t = float(changes[2].split(' ')[0])

                yield self.t(
                    'searchentry',
                    parity = count & 1,
                    author = changes[1],
                    parent = self.parents("changelogparent",
                                          cl.parents(n), cl.rev),
                    changelogtag = self.showtag("changelogtag",n),
                    p1 = hex(p1), p2 = hex(p2),
                    p1rev = cl.rev(p1), p2rev = cl.rev(p2),
                    manifest = hex(changes[0]),
                    desc = changes[4],
                    date = t,
                    files = self.listfilediffs(changes[3], n),
                    rev = i,
                    node = hn)

                if count >= self.maxchanges: break

        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

        yield self.t('search',
                     header = self.header(),
                     footer = self.footer(),
                     query = query,
                     repo = self.reponame,
                     manifest = hex(mf),
                     entries = changelist)

    def changeset(self, nodeid):
        n = bin(nodeid)
        cl = self.repo.changelog
        changes = cl.read(n)
        p1, p2 = cl.parents(n)
        p1rev, p2rev = cl.rev(p1), cl.rev(p2)
        t = float(changes[2].split(' ')[0])

        files = []
        mf = self.repo.manifest.read(changes[0])
        for f in changes[3]:
            files.append(self.t("filenodelink",
                                filenode = hex(mf.get(f, nullid)), file = f))

        def diff():
            yield self.diff(p1, n, changes[3])

        yield self.t('changeset',
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     diff = diff,
                     rev = cl.rev(n),
                     node = nodeid,
                     parent = self.parents("changesetparent",
                                           cl.parents(n), cl.rev),
                     changesettag = self.showtag("changesettag",n),
                     p1 = hex(p1), p2 = hex(p2),
                     p1rev = cl.rev(p1), p2rev = cl.rev(p2),
                     manifest = hex(changes[0]),
                     author = changes[1],
                     desc = changes[4],
                     date = t,
                     files = files)

    def filelog(self, f, filenode):
        cl = self.repo.changelog
        fl = self.repo.file(f)
        count = fl.count()

        def entries():
            l = []
            parity = (count - 1) & 1

            for i in range(count):

                n = fl.node(i)
                lr = fl.linkrev(n)
                cn = cl.node(lr)
                cs = cl.read(cl.node(lr))
                p1, p2 = fl.parents(n)
                t = float(cs[2].split(' ')[0])

                l.insert(0, self.t("filelogentry",
                                   parity = parity,
                                   filenode = hex(n),
                                   filerev = i,
                                   file = f,
                                   node = hex(cn),
                                   author = cs[1],
                                   date = t,
                                   desc = cs[4],
                                   p1 = hex(p1), p2 = hex(p2),
                                   p1rev = fl.rev(p1), p2rev = fl.rev(p2)))
                parity = 1 - parity

            yield l

        yield self.t("filelog",
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     file = f,
                     filenode = filenode,
                     entries = entries)

    def filerevision(self, f, node):
        fl = self.repo.file(f)
        n = bin(node)
        text = fl.read(n)
        changerev = fl.linkrev(n)
        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        p1, p2 = fl.parents(n)
        t = float(cs[2].split(' ')[0])
        mfn = cs[0]

        def lines():
            for l, t in enumerate(text.splitlines(1)):
                yield self.t("fileline", line = t,
                             linenumber = "% 6d" % (l + 1),
                             parity = l & 1)

        yield self.t("filerevision", file = f,
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     filenode = node,
                     path = up(f),
                     text = lines(),
                     rev = changerev,
                     node = hex(cn),
                     manifest = hex(mfn),
                     author = cs[1],
                     date = t,
                     parent = self.parents("filerevparent",
                                           fl.parents(n), fl.rev, file=f),
                     p1 = hex(p1), p2 = hex(p2),
                     permissions = self.repo.manifest.readflags(mfn)[f],
                     p1rev = fl.rev(p1), p2rev = fl.rev(p2))

    def fileannotate(self, f, node):
        bcache = {}
        ncache = {}
        fl = self.repo.file(f)
        n = bin(node)
        changerev = fl.linkrev(n)

        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        p1, p2 = fl.parents(n)
        t = float(cs[2].split(' ')[0])
        mfn = cs[0]

        def annotate():
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

                yield self.t("annotateline",
                             parity = parity,
                             node = hex(cnode),
                             rev = r,
                             author = name,
                             file = f,
                             line = l)

        yield self.t("fileannotate",
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     file = f,
                     filenode = node,
                     annotate = annotate,
                     path = up(f),
                     rev = changerev,
                     node = hex(cn),
                     manifest = hex(mfn),
                     author = cs[1],
                     date = t,
                     parent = self.parents("fileannotateparent",
                                           fl.parents(n), fl.rev, file=f),
                     p1 = hex(p1), p2 = hex(p2),
                     permissions = self.repo.manifest.readflags(mfn)[f],
                     p1rev = fl.rev(p1), p2rev = fl.rev(p2))

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

        def filelist():
            parity = 0
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if fnode:
                    yield self.t("manifestfileentry",
                                 file = full,
                                 manifest = mnode,
                                 filenode = hex(fnode),
                                 parity = parity,
                                 basename = f,
                                 permissions = mff[full])
                else:
                    yield self.t("manifestdirentry",
                                 parity = parity,
                                 path = os.path.join(path, f),
                                 manifest = mnode, basename = f[:-1])
                parity = 1 - parity

        yield self.t("manifest",
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     manifest = mnode,
                     rev = rev,
                     node = hex(node),
                     path = path,
                     up = up(path),
                     entries = filelist)

    def tags(self):
        cl = self.repo.changelog
        mf = cl.read(cl.tip())[0]

        i = self.repo.tagslist()
        i.reverse()

        def entries():
            parity = 0
            for k,n in i:
                yield self.t("tagentry",
                             parity = parity,
                             tag = k,
                             node = hex(n))
                parity = 1 - parity

        yield self.t("tags",
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     manifest = hex(mf),
                     entries = entries)

    def filediff(self, file, changeset):
        n = bin(changeset)
        cl = self.repo.changelog
        p1 = cl.parents(n)[0]
        cs = cl.read(n)
        mf = self.repo.manifest.read(cs[0])

        def diff():
            yield self.diff(p1, n, file)

        yield self.t("filediff",
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     file = file,
                     filenode = hex(mf.get(file, nullid)),
                     node = changeset,
                     rev = self.repo.changelog.rev(n),
                     parent = self.parents("filediffparent",
                              cl.parents(n), cl.rev),
                     p1rev = self.repo.changelog.rev(p1),
                     diff = diff)

    # add tags to things
    # tags -> list of changesets corresponding to tags
    # find tag, changeset, file

    def run(self):
        self.refresh()
        args = cgi.parse()

        m = os.path.join(self.templates, "map")
        if args.has_key('style'):
            b = os.path.basename("map-" + args['style'][0])
            p = os.path.join(self.templates, b)
            if os.path.isfile(p): m = p

        self.t = templater(m, self.filters)

        if not args.has_key('cmd') or args['cmd'][0] == 'changelog':
            c = self.repo.changelog.count() - 1
            hi = c
            if args.has_key('rev'):
                hi = args['rev'][0]
                try:
                    hi = self.repo.changelog.rev(self.repo.lookup(hi))
                except KeyError:
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
            httphdr("text/plain")
            h = self.repo.heads()
            sys.stdout.write(" ".join(map(hex, h)) + "\n")

        elif args['cmd'][0] == 'branches':
            httphdr("text/plain")
            nodes = []
            if args.has_key('nodes'):
                nodes = map(bin, args['nodes'][0].split(" "))
            for b in self.repo.branches(nodes):
                sys.stdout.write(" ".join(map(hex, b)) + "\n")

        elif args['cmd'][0] == 'between':
            httphdr("text/plain")
            nodes = []
            if args.has_key('pairs'):
                pairs = [ map(bin, p.split("-"))
                          for p in args['pairs'][0].split(" ") ]
            for b in self.repo.between(pairs):
                sys.stdout.write(" ".join(map(hex, b)) + "\n")

        elif args['cmd'][0] == 'changegroup':
            httphdr("application/hg-changegroup")
            nodes = []
            if self.viewonly:
                return

            if args.has_key('roots'):
                nodes = map(bin, args['roots'][0].split(" "))

            z = zlib.compressobj()
            for chunk in self.repo.changegroup(nodes):
                sys.stdout.write(z.compress(chunk))

            sys.stdout.write(z.flush())

        else:
            write(self.t("error"))

def server(path, name, templates, address, port):

    import BaseHTTPServer
    import sys, os

    class hgwebhandler(BaseHTTPServer.BaseHTTPRequestHandler):
        def do_POST(self):
            try:
                self.do_hgweb()
            except socket.error, inst:
                if inst.args[0] != 32: raise

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

    hg = hgweb(path, name, templates)
    httpd = BaseHTTPServer.HTTPServer((address, port), hgwebhandler)
    httpd.serve_forever()
