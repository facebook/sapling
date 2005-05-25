#!/usr/bin/env python
#
# hgweb.py - 0.2 - 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
#    - web interface to a mercurial repository
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

# useful for debugging
import cgitb
cgitb.enable()

import os, cgi, time, re, difflib, sys, zlib
from mercurial.hg import *

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
        if n >= 1: return fmt(t, n)

def nl2br(text):
    return text.replace('\n', '<br/>')

def obfuscate(text):
    return ''.join([ '&#%d' % ord(c) for c in text ])

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

def template(tmpl, **map):
    while tmpl:
        m = re.search(r"#([a-zA-Z0-9]+)#", tmpl)
        if m:
            yield tmpl[:m.start(0)]
            v = map.get(m.group(1), "")
            yield callable(v) and v() or v
            tmpl = tmpl[m.end(0):]
        else:
            yield tmpl
            return

class templater:
    def __init__(self, mapfile):
        self.cache = {}
        self.map = {}
        self.base = os.path.dirname(mapfile)
        
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
        return template(tmpl, **map)
        
class hgweb:
    maxchanges = 20
    maxfiles = 10

    def __init__(self, path, name, templatemap):
        self.reponame = name
        self.repo = repository(ui(), path)
        self.t = templater(templatemap)

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

    def parent(self, t1, node=nullid, rev=-1, **args):
        if node != hex(nullid):
            yield self.t(t1, node = node, rev = rev, **args)

    def diff(self, node1, node2, files):
        def filterfiles(list, files):
            l = [ x for x in list if x in files ]
            
            for f in files:
                if f[-1] != os.sep: f += os.sep
                l += [ x for x in list if x.startswith(f) ]
            return l

        def prettyprint(diff):
            for l in diff.splitlines(1):
                line = cgi.escape(l)
                if line.startswith('+'):
                    yield self.t("difflineplus", line = line)
                elif line.startswith('-'):
                    yield self.t("difflineminus", line = line)
                elif line.startswith('@'):
                    yield self.t("difflineat", line = line)
                else:
                    yield self.t("diffline", line = line)

        r = self.repo
        cl = r.changelog
        mf = r.manifest
        change1 = cl.read(node1)
        change2 = cl.read(node2)
        mmap1 = mf.read(change1[0])
        mmap2 = mf.read(change2[0])
        date1 = self.date(change1)
        date2 = self.date(change2)

        c, a, d = r.diffrevs(node1, node2)
        c, a, d = map(lambda x: filterfiles(x, files), (c, a, d))

        for f in c:
            to = r.file(f).read(mmap1[f])
            tn = r.file(f).read(mmap2[f])
            yield prettyprint(mdiff.unidiff(to, date1, tn, date2, f))
        for f in a:
            to = ""
            tn = r.file(f).read(mmap2[f])
            yield prettyprint(mdiff.unidiff(to, date1, tn, date2, f))
        for f in d:
            to = r.file(f).read(mmap1[f])
            tn = ""
            yield prettyprint(mdiff.unidiff(to, date1, tn, date2, f))

    def header(self):
        yield self.t("header", repo = self.reponame)

    def footer(self):
        yield self.t("footer", repo = self.reponame)

    def changelog(self, pos=None):
        def changenav():
            def seq(factor = 1):
                yield 1 * factor
                yield 2 * factor
                yield 5 * factor
                for f in seq(factor * 10):
                    yield f
                    
            linear = range(0, count - 2, self.maxchanges)[0:8]

            for i in linear:
                yield self.t("naventry", rev = max(i, 1))

            for s in seq():
                if s > count - 2: break
                if s > linear[-1]:
                    yield self.t("naventry", rev = s)
                    
            yield self.t("naventry", rev = count - 1)

        def changelist():
            parity = (start - end) & 1
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in range(start, end + 1):
                n = cl.node(i)
                changes = cl.read(n)
                hn = hex(n)
                p1, p2 = cl.parents(n)
                t = float(changes[2].split(' ')[0])

                l.insert(0, self.t(
                    'changelogentry',
                    parity = parity,
                    author = obfuscate(changes[1]),
                    shortdesc = cgi.escape(changes[4].splitlines()[0]),
                    age = age(t),
                    parent1 = self.parent("changelogparent",
                                          hex(p1), cl.rev(p1)),
                    parent2 = self.parent("changelogparent",
                                          hex(p2), cl.rev(p2)),
                    p1 = hex(p1), p2 = hex(p2),
                    p1rev = cl.rev(p1), p2rev = cl.rev(p2),
                    manifest = hex(changes[0]),
                    desc = nl2br(cgi.escape(changes[4])),
                    date = time.asctime(time.gmtime(t)),
                    files = self.listfilediffs(changes[3], n),
                    rev = i,
                    node = hn))
                parity = 1 - parity

            yield l

        count = self.repo.changelog.count()
        pos = pos or count - 1
        end = min(pos, count - 1)
        start = max(0, pos - self.maxchanges)
        end = min(count - 1, start + self.maxchanges)

        yield self.t('changelog',
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     changenav = changenav,
                     rev = pos, changesets = count, entries = changelist)

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
                                filenode = hex(mf[f]), file = f))

        def diff():
            yield self.diff(p1, n, changes[3])

        yield self.t('changeset',
                     header = self.header(),
                     footer = self.footer(),
                     repo = self.reponame,
                     diff = diff,
                     rev = cl.rev(n),
                     node = nodeid,
                     shortdesc = cgi.escape(changes[4].splitlines()[0]),
                     parent1 = self.parent("changesetparent",
                                           hex(p1), cl.rev(p1)),
                     parent2 = self.parent("changesetparent",
                                           hex(p2), cl.rev(p2)),
                     p1 = hex(p1), p2 = hex(p2),
                     p1rev = cl.rev(p1), p2rev = cl.rev(p2),
                     manifest = hex(changes[0]),
                     author = obfuscate(changes[1]),
                     desc = nl2br(cgi.escape(changes[4])),
                     date = time.asctime(time.gmtime(t)),
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
                                   author = obfuscate(cs[1]),
                                   age = age(t),
                                   date = time.asctime(time.gmtime(t)),
                                   shortdesc = cgi.escape(cs[4].splitlines()[0]),
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
        text = cgi.escape(fl.read(n))
        changerev = fl.linkrev(n)
        cl = self.repo.changelog
        cn = cl.node(changerev)
        cs = cl.read(cn)
        p1, p2 = fl.parents(n)
        t = float(cs[2].split(' ')[0])
        mfn = cs[0]

        def lines():
            for l, t in enumerate(text.splitlines(1)):
                yield self.t("fileline",
                             line = t,
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
                     author = obfuscate(cs[1]),
                     age = age(t),
                     date = time.asctime(time.gmtime(t)),
                     shortdesc = cgi.escape(cs[4].splitlines()[0]),
                     parent1 = self.parent("filerevparent",
                                           hex(p1), fl.rev(p1), file=f),
                     parent2 = self.parent("filerevparent",
                                           hex(p2), fl.rev(p2), file=f),
                     p1 = hex(p1), p2 = hex(p2),
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
                             line = cgi.escape(l))

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
                     author = obfuscate(cs[1]),
                     age = age(t),
                     date = time.asctime(time.gmtime(t)),
                     shortdesc = cgi.escape(cs[4].splitlines()[0]),
                     parent1 = self.parent("fileannotateparent",
                                           hex(p1), fl.rev(p1), file=f),
                     parent2 = self.parent("fileannotateparent",
                                           hex(p2), fl.rev(p2), file=f),
                     p1 = hex(p1), p2 = hex(p2),
                     p1rev = fl.rev(p1), p2rev = fl.rev(p2))

    def manifest(self, mnode, path):
        mf = self.repo.manifest.read(bin(mnode))
        rev = self.repo.manifest.rev(bin(mnode))
        node = self.repo.changelog.node(rev)

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
                                 basename = f)
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
                     filenode = hex(mf[file]),
                     node = changeset,
                     rev = self.repo.changelog.rev(n),
                     p1 = hex(p1),
                     p1rev = self.repo.changelog.rev(p1),
                     diff = diff)
                     
    # add tags to things
    # tags -> list of changesets corresponding to tags
    # find tag, changeset, file

    def run(self):
        args = cgi.parse()

        if not args.has_key('cmd') or args['cmd'][0] == 'changelog':
            hi = self.repo.changelog.count()
            if args.has_key('rev'):
                hi = int(args['rev'][0])

            write(self.changelog(hi))
            
        elif args['cmd'][0] == 'changeset':
            write(self.changeset(args['node'][0]))

        elif args['cmd'][0] == 'manifest':
            write(self.manifest(args['manifest'][0], args['path'][0]))

        elif args['cmd'][0] == 'filediff':
            write(self.filediff(args['file'][0], args['node'][0]))

        elif args['cmd'][0] == 'file':
            write(self.filerevision(args['file'][0], args['filenode'][0]))

        elif args['cmd'][0] == 'annotate':
            write(self.fileannotate(args['file'][0], args['filenode'][0]))

        elif args['cmd'][0] == 'filelog':
            write(self.filelog(args['file'][0], args['filenode'][0]))

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
            if args.has_key('roots'):
                nodes = map(bin, args['roots'][0].split(" "))

            z = zlib.compressobj()
            for chunk in self.repo.changegroup(nodes):
                sys.stdout.write(z.compress(chunk))

            sys.stdout.write(z.flush())

        else:
            write(self.t("error"))

if __name__ == "__main__":
    hgweb().run()
