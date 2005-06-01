# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import sys, struct, sha, socket, os, time, re, urllib2
import urllib
from mercurial import byterange, lock
from mercurial.transaction import *
from mercurial.revlog import *
from difflib import SequenceMatcher

class filelog(revlog):
    def __init__(self, opener, path):
        revlog.__init__(self, opener,
                        os.path.join("data", path + ".i"),
                        os.path.join("data", path + ".d"))

    def read(self, node):
        return self.revision(node)
    def add(self, text, transaction, link, p1=None, p2=None):
        return self.addrevision(text, transaction, link, p1, p2)

    def annotate(self, node):

        def decorate(text, rev):
            return [(rev, l) for l in text.splitlines(1)]

        def strip(annotation):
            return [e[1] for e in annotation]

        def pair(parent, child):
            new = []
            sm = SequenceMatcher(None, strip(parent), strip(child))
            for o, m, n, s, t in sm.get_opcodes():
                if o == 'equal':
                    new += parent[m:n]
                else:
                    new += child[s:t]
            return new

        # find all ancestors
        needed = {}
        visit = [node]
        while visit:
            n = visit.pop(0)
            for p in self.parents(n):
                if p not in needed:
                    needed[p] = 1
                    visit.append(p)
                else:
                    # count how many times we'll use this
                    needed[p] += 1

        # sort by revision which is a topological order
        visit = needed.keys()
        visit = [ (self.rev(n), n) for n in visit ]
        visit.sort()
        visit = [ p[1] for p in visit ]
        hist = {}

        for n in visit:
            curr = decorate(self.read(n), self.linkrev(n))
            for p in self.parents(n):
                if p != nullid:
                    curr = pair(hist[p], curr)
                    # trim the history of unneeded revs
                    needed[p] -= 1
                    if not needed[p]:
                        del hist[p]
            hist[n] = curr

        return hist[n]

class manifest(revlog):
    def __init__(self, opener):
        self.mapcache = None
        self.listcache = None
        self.addlist = None
        revlog.__init__(self, opener, "00manifest.i", "00manifest.d")

    def read(self, node):
        if self.mapcache and self.mapcache[0] == node:
            return self.mapcache[1].copy()
        text = self.revision(node)
        map = {}
        self.listcache = (text, text.splitlines(1))
        for l in self.listcache[1]:
            (f, n) = l.split('\0')
            map[f] = bin(n[:40])
        self.mapcache = (node, map)
        return map

    def diff(self, a, b):
        # this is sneaky, as we're not actually using a and b
        if self.listcache and self.addlist and self.listcache[0] == a:
            d = mdiff.diff(self.listcache[1], self.addlist, 1)
            if mdiff.patch(a, d) != b:
                sys.stderr.write("*** sortdiff failed, falling back ***\n")
                return mdiff.textdiff(a, b)
            return d
        else:
            return mdiff.textdiff(a, b)

    def add(self, map, transaction, link, p1=None, p2=None):
        files = map.keys()
        files.sort()

        self.addlist = ["%s\000%s\n" % (f, hex(map[f])) for f in files]
        text = "".join(self.addlist)

        n = self.addrevision(text, transaction, link, p1, p2)
        self.mapcache = (n, map)
        self.listcache = (text, self.addlist)
        self.addlist = None

        return n

class changelog(revlog):
    def __init__(self, opener):
        revlog.__init__(self, opener, "00changelog.i", "00changelog.d")

    def extract(self, text):
        if not text:
            return (nullid, "", "0", [], "")
        last = text.index("\n\n")
        desc = text[last + 2:]
        l = text[:last].splitlines()
        manifest = bin(l[0])
        user = l[1]
        date = l[2]
        files = l[3:]
        return (manifest, user, date, files, desc)

    def read(self, node):
        return self.extract(self.revision(node))

    def add(self, manifest, list, desc, transaction, p1=None, p2=None,
                  user=None, date=None):
        user = (user or
                os.environ.get("HGUSER") or
                os.environ.get("EMAIL") or
                os.environ.get("LOGNAME", "unknown") + '@' + socket.getfqdn())
        date = date or "%d %d" % (time.time(), time.timezone)
        list.sort()
        l = [hex(manifest), user, date] + list + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, self.count(), p1, p2)

class dircache:
    def __init__(self, opener, ui):
        self.opener = opener
        self.dirty = 0
        self.ui = ui
        self.map = None
    def __del__(self):
        if self.dirty: self.write()
    def __getitem__(self, key):
        try:
            return self.map[key]
        except TypeError:
            self.read()
            return self[key]
        
    def read(self):
        if self.map is not None: return self.map

        self.map = {}
        try:
            st = self.opener("dircache").read()
        except: return

        pos = 0
        while pos < len(st):
            e = struct.unpack(">llll", st[pos:pos+16])
            l = e[3]
            pos += 16
            f = st[pos:pos + l]
            self.map[f] = e[:3]
            pos += l
        
    def update(self, files):
        if not files: return
        self.read()
        self.dirty = 1
        for f in files:
            try:
                s = os.stat(f)
                self.map[f] = (s.st_mode, s.st_size, s.st_mtime)
            except IOError:
                self.remove(f)

    def taint(self, files):
        if not files: return
        self.read()
        self.dirty = 1
        for f in files:
            self.map[f] = (0, -1, 0)

    def remove(self, files):
        if not files: return
        self.read()
        self.dirty = 1
        for f in files:
            try:
                del self.map[f]
            except KeyError:
                self.ui.warn("Not in dircache: %s\n" % f)
                pass

    def clear(self):
        self.map = {}
        self.dirty = 1

    def write(self):
        st = self.opener("dircache", "w")
        for f, e in self.map.items():
            e = struct.pack(">llll", e[0], e[1], e[2], len(f))
            st.write(e + f)
        self.dirty = 0

    def copy(self):
        self.read()
        return self.map.copy()

# used to avoid circular references so destructors work
def opener(base):
    p = base
    def o(path, mode="r"):
        if p[:7] == "http://":
            f = os.path.join(p, urllib.quote(path))
            return httprangereader(f)

        f = os.path.join(p, path)

        if mode != "r":
            try:
                s = os.stat(f)
            except OSError:
                d = os.path.dirname(f)
                if not os.path.isdir(d):
                    os.makedirs(d)
            else:
                if s.st_nlink > 1:
                    file(f + ".tmp", "w").write(file(f).read())
                    os.rename(f+".tmp", f)

        return file(f, mode)

    return o

class localrepository:
    def __init__(self, ui, path=None, create=0):
        self.remote = 0
        if path and path[:7] == "http://":
            self.remote = 1
            self.path = path
        else:
            if not path:
                p = os.getcwd()
                while not os.path.isdir(os.path.join(p, ".hg")):
                    p = os.path.dirname(p)
                    if p == "/": raise "No repo found"
                path = p
            self.path = os.path.join(path, ".hg")

        self.root = path
        self.ui = ui

        if create:
            os.mkdir(self.path)  
            os.mkdir(self.join("data"))

        self.opener = opener(self.path)
        self.manifest = manifest(self.opener)
        self.changelog = changelog(self.opener)
        self.ignorelist = None
        self.tags = None

        if not self.remote:
            self.dircache = dircache(self.opener, ui)
            try:
                self.current = bin(self.opener("current").read())
            except IOError:
                self.current = None

    def setcurrent(self, node):
        self.current = node
        self.opener("current", "w").write(hex(node))
      
    def ignore(self, f):
        if self.ignorelist is None:
            self.ignorelist = []
            try:
                l = open(os.path.join(self.root, ".hgignore"))
                for pat in l:
                    if pat != "\n":
                        self.ignorelist.append(re.compile(pat[:-1]))
            except IOError: pass
        for pat in self.ignorelist:
            if pat.search(f): return True
        return False

    def lookup(self, key):
        if self.tags is None:
            self.tags = {}
            try:
                fl = self.file(".hgtags")
                for l in fl.revision(fl.tip()).splitlines():
                    if l:
                        n, k = l.split(" ")
                        self.tags[k] = bin(n)
            except KeyError: pass
        try:
            return self.tags[key]
        except KeyError:
            return self.changelog.lookup(key)

    def join(self, f):
        return os.path.join(self.path, f)

    def file(self, f):
        if f[0] == '/': f = f[1:]
        return filelog(self.opener, f)

    def transaction(self):
        return transaction(self.opener, self.join("journal"),
                           self.join("undo"))

    def recover(self, f = "journal"):
        self.lock()
        if os.path.exists(self.join(f)):
            self.ui.status("attempting to rollback %s information\n" % f)
            return rollback(self.opener, self.join(f))
        else:
            self.ui.warn("no %s information available\n" % f)

    def lock(self, wait = 1):
        try:
            return lock.lock(self.join("lock"), 0)
        except lock.LockHeld, inst:
            if wait:
                self.ui.warn("waiting for lock held by %s\n" % inst.args[0])
                return lock.lock(self.join("lock"), wait)
            raise inst

    def rawcommit(self, files, text, user, date, p1=None, p2=None):
        p1 = p1 or self.current or nullid
        pchange = self.changelog.read(p1)
        pmmap = self.manifest.read(pchange[0])
        tr = self.transaction()
        mmap = {}
        linkrev = self.changelog.count()
        for f in files:
            try:
                t = file(f).read()
            except IOError:
                self.ui.warn("Read file %s error, skipped\n" % f)
                continue
            r = self.file(f)
            prev = pmmap.get(f, nullid)
            mmap[f] = r.add(t, tr, linkrev, prev)

        mnode = self.manifest.add(mmap, tr, linkrev, pchange[0])
        n = self.changelog.add(mnode, files, text, tr, p1, p2, user ,date, )
        tr.close()
        self.setcurrent(n)
        self.dircache.clear()
        self.dircache.update(mmap)

    def commit(self, parent, update = None, text = ""):
        self.lock()
        try:
            remove = [ l[:-1] for l in self.opener("to-remove") ]
            os.unlink(self.join("to-remove"))

        except IOError:
            remove = []

        if update == None:
            update = self.diffdir(self.root, parent)[0]

        if not update:
            self.ui.status("nothing changed\n")
            return

        tr = self.transaction()

        # check in files
        new = {}
        linkrev = self.changelog.count()
        update.sort()
        for f in update:
            self.ui.note(f + "\n")
            try:
                t = file(f).read()
            except IOError:
                remove.append(f)
                continue
            r = self.file(f)
            new[f] = r.add(t, tr, linkrev)

        # update manifest
        mmap = self.manifest.read(self.manifest.tip())
        mmap.update(new)
        for f in remove:
            del mmap[f]
        mnode = self.manifest.add(mmap, tr, linkrev)

        # add changeset
        new = new.keys()
        new.sort()

        edittext = text + "\n" + "HG: manifest hash %s\n" % hex(mnode)
        edittext += "".join(["HG: changed %s\n" % f for f in new])
        edittext += "".join(["HG: removed %s\n" % f for f in remove])
        edittext = self.ui.edit(edittext)

        n = self.changelog.add(mnode, new, edittext, tr)
        tr.close()

        self.setcurrent(n)
        self.dircache.update(new)
        self.dircache.remove(remove)

    def checkout(self, node):
        # checkout is really dumb at the moment
        # it ought to basically merge
        change = self.changelog.read(node)
        l = self.manifest.read(change[0]).items()
        l.sort()

        for f,n in l:
            if f[0] == "/": continue
            self.ui.note(f, "\n")
            t = self.file(f).revision(n)
            try:
                file(f, "w").write(t)
            except IOError:
                os.makedirs(os.path.dirname(f))
                file(f, "w").write(t)

        self.setcurrent(node)
        self.dircache.clear()
        self.dircache.update([f for f,n in l])

    def diffdir(self, path, changeset):
        changed = []
        mf = {}
        added = []

        if changeset:
            change = self.changelog.read(changeset)
            mf = self.manifest.read(change[0])

        if changeset == self.current:
            dc = self.dircache.copy()
        else:
            dc = dict.fromkeys(mf)

        def fcmp(fn):
            t1 = file(os.path.join(self.root, fn)).read()
            t2 = self.file(fn).revision(mf[fn])
            return cmp(t1, t2)

        for dir, subdirs, files in os.walk(self.root):
            d = dir[len(self.root)+1:]
            if ".hg" in subdirs: subdirs.remove(".hg")
            
            for f in files:
                fn = os.path.join(d, f)
                try: s = os.stat(os.path.join(self.root, fn))
                except: continue
                if fn in dc:
                    c = dc[fn]
                    del dc[fn]
                    if not c:
                        if fcmp(fn):
                            changed.append(fn)
                    elif c[1] != s.st_size:
                        changed.append(fn)
                    elif c[0] != s.st_mode or c[2] != s.st_mtime:
                        if fcmp(fn):
                            changed.append(fn)
                else:
                    if self.ignore(fn): continue
                    added.append(fn)

        deleted = dc.keys()
        deleted.sort()

        return (changed, added, deleted)

    def diffrevs(self, node1, node2):
        changed, added = [], []

        change = self.changelog.read(node1)
        mf1 = self.manifest.read(change[0])
        change = self.changelog.read(node2)
        mf2 = self.manifest.read(change[0])

        for fn in mf2:
            if mf1.has_key(fn):
                if mf1[fn] != mf2[fn]:
                    changed.append(fn)
                del mf1[fn]
            else:
                added.append(fn)
                
        deleted = mf1.keys()
        deleted.sort()
    
        return (changed, added, deleted)

    def add(self, list):
        self.dircache.taint(list)

    def remove(self, list):
        dl = self.opener("to-remove", "a")
        for f in list:
            dl.write(f + "\n")

    def branches(self, nodes):
        if not nodes: nodes = [self.changelog.tip()]
        b = []
        for n in nodes:
            t = n
            while n:
                p = self.changelog.parents(n)
                if p[1] != nullid or p[0] == nullid:
                    b.append((t, n, p[0], p[1]))
                    break
                n = p[0]
        return b

    def between(self, pairs):
        r = []

        for top, bottom in pairs:
            n, l, i = top, [], 0
            f = 1

            while n != bottom:
                p = self.changelog.parents(n)[0]
                if i == f:
                    l.append(n) 
                    f = f * 2
                n = p
                i += 1

            r.append(l)

        return r

    def newer(self, nodes):
        m = {}
        nl = []
        pm = {}
        cl = self.changelog
        t = l = cl.count()

        # find the lowest numbered node
        for n in nodes:
            l = min(l, cl.rev(n))
            m[n] = 1

        for i in xrange(l, t):
            n = cl.node(i)
            if n in m: # explicitly listed
                pm[n] = 1
                nl.append(n)
                continue
            for p in cl.parents(n):
                if p in pm: # parent listed
                    pm[n] = 1
                    nl.append(n)
                    break

        return nl

    def getchangegroup(self, remote):
        m = self.changelog.nodemap
        search = []
        fetch = []
        seen = {}
        seenbranch = {}

        self.ui.status("searching for changes\n")
        tip = remote.branches([])[0]
        self.ui.debug("remote tip branch is %s:%s\n" %
                      (short(tip[0]), short(tip[1])))

        # if we have an empty repo, fetch everything
        if self.changelog.tip() == nullid:
            return remote.changegroup([nullid])

        # otherwise, assume we're closer to the tip than the root
        unknown = [tip]

        if tip[0] in m:
            self.ui.status("nothing to do!\n")
            return None

        while unknown:
            n = unknown.pop(0)
            seen[n[0]] = 1
            
            self.ui.debug("examining %s:%s\n" % (short(n[0]), short(n[1])))
            if n == nullid: break
            if n in seenbranch:
                self.ui.debug("branch already found\n")
                continue
            if n[1] and n[1] in m: # do we know the base?
                self.ui.debug("found incomplete branch %s:%s\n"
                              % (short(n[0]), short(n[1])))
                search.append(n) # schedule branch range for scanning
                seenbranch[n] = 1
            else:
                if n[2] in m and n[3] in m:
                    if n[1] not in fetch:
                        self.ui.debug("found new changeset %s\n" %
                                      short(n[1]))
                        fetch.append(n[1]) # earliest unknown
                        continue

                r = []
                for a in n[2:4]:
                    if a not in seen: r.append(a)
                    
                if r:
                    self.ui.debug("requesting %s\n" %
                                " ".join(map(short, r)))
                    for b in remote.branches(r):
                        self.ui.debug("received %s:%s\n" %
                                      (short(b[0]), short(b[1])))
                        if b[0] not in m and b[0] not in seen:
                            unknown.append(b)
  
        while search:
            n = search.pop(0)
            l = remote.between([(n[0], n[1])])[0]
            p = n[0]
            f = 1
            for i in l + [n[1]]:
                if i in m:
                    if f <= 2:
                        self.ui.debug("found new branch changeset %s\n" %
                                          short(p))
                        fetch.append(p)
                    else:
                        self.ui.debug("narrowed branch search to %s:%s\n"
                                      % (short(p), short(i)))
                        search.append((p, i))
                    break
                p, f = i, f * 2

        for f in fetch:
            if f in m:
                raise "already have", short(f[:4])

        self.ui.note("adding new changesets starting at " +
                     " ".join([short(f) for f in fetch]) + "\n")

        return remote.changegroup(fetch)
    
    def changegroup(self, basenodes):
        nodes = self.newer(basenodes)

        # construct the link map
        linkmap = {}
        for n in nodes:
            linkmap[self.changelog.rev(n)] = n

        # construct a list of all changed files
        changed = {}
        for n in nodes:
            c = self.changelog.read(n)
            for f in c[3]:
                changed[f] = 1
        changed = changed.keys()
        changed.sort()

        # the changegroup is changesets + manifests + all file revs
        revs = [ self.changelog.rev(n) for n in nodes ]

        for y in self.changelog.group(linkmap): yield y
        for y in self.manifest.group(linkmap): yield y
        for f in changed:
            yield struct.pack(">l", len(f) + 4) + f
            g = self.file(f).group(linkmap)
            for y in g:
                yield y

    def addchangegroup(self, generator):
        changesets = files = revisions = 0

        self.lock()
        class genread:
            def __init__(self, generator):
                self.g = generator
                self.buf = ""
            def read(self, l):
                while l > len(self.buf):
                    try:
                        self.buf += self.g.next()
                    except StopIteration:
                        break
                d, self.buf = self.buf[:l], self.buf[l:]
                return d
                
        if not generator: return
        source = genread(generator)

        def getchunk():
            d = source.read(4)
            if not d: return ""
            l = struct.unpack(">l", d)[0]
            if l <= 4: return ""
            return source.read(l - 4)

        def getgroup():
            while 1:
                c = getchunk()
                if not c: break
                yield c

        tr = self.transaction()
        simple = True
        need = {}

        self.ui.status("adding changesets\n")
        # pull off the changeset group
        def report(x):
            self.ui.debug("add changeset %s\n" % short(x))
            return self.changelog.count()

        co = self.changelog.tip()
        cn = self.changelog.addgroup(getgroup(), report, tr)

        changesets = self.changelog.rev(cn) - self.changelog.rev(co)

        self.ui.status("adding manifests\n")
        # pull off the manifest group
        mm = self.manifest.tip()
        mo = self.manifest.addgroup(getgroup(),
                                    lambda x: self.changelog.rev(x), tr)

        # do we need a resolve?
        if self.changelog.ancestor(co, cn) != co:
            simple = False
            resolverev = self.changelog.count()

            # resolve the manifest to determine which files
            # we care about merging
            self.ui.status("resolving manifests\n")
            ma = self.manifest.ancestor(mm, mo)
            omap = self.manifest.read(mo) # other
            amap = self.manifest.read(ma) # ancestor
            mmap = self.manifest.read(mm) # mine
            nmap = {}

            self.ui.debug(" ancestor %s local %s remote %s\n" %
                          (short(ma), short(mm), short(mo)))

            for f, mid in mmap.iteritems():
                if f in omap:
                    if mid != omap[f]:
                        self.ui.debug(" %s versions differ, do resolve\n" % f)
                        need[f] = mid # use merged version or local version
                    else:
                        nmap[f] = mid # keep ours
                    del omap[f]
                elif f in amap:
                    if mid != amap[f]:
                        r = self.ui.prompt(
                            (" local changed %s which remote deleted\n" % f) +
                            "(k)eep or (d)elete?", "[kd]", "k")
                        if r == "k": nmap[f] = mid
                    else:
                        self.ui.debug("other deleted %s\n" % f)
                        pass # other deleted it
                else:
                    self.ui.debug("local created %s\n" %f)
                    nmap[f] = mid # we created it

            del mmap

            for f, oid in omap.iteritems():
                if f in amap:
                    if oid != amap[f]:
                        r = self.ui.prompt(
                            ("remote changed %s which local deleted\n" % f) +
                            "(k)eep or (d)elete?", "[kd]", "k")
                        if r == "k": nmap[f] = oid
                    else:
                        pass # probably safe
                else:
                    self.ui.debug("remote created %s, do resolve\n" % f)
                    need[f] = oid

            del omap
            del amap

        new = need.keys()
        new.sort()

        # process the files
        self.ui.status("adding files\n")
        while 1:
            f = getchunk()
            if not f: break
            self.ui.debug("adding %s revisions\n" % f)
            fl = self.file(f)
            o = fl.tip()
            n = fl.addgroup(getgroup(), lambda x: self.changelog.rev(x), tr)
            revisions += fl.rev(n) - fl.rev(o)
            files += 1
            if f in need:
                del need[f]
                # manifest resolve determined we need to merge the tips
                nmap[f] = self.merge3(fl, f, o, n, tr, resolverev)

        if need:
            # we need to do trivial merges on local files
            for f in new:
                if f not in need: continue
                fl = self.file(f)
                nmap[f] = self.merge3(fl, f, need[f], fl.tip(), tr, resolverev)
                revisions += 1

        # For simple merges, we don't need to resolve manifests or changesets
        if simple:
            self.ui.debug("simple merge, skipping resolve\n")
            self.ui.status(("modified %d files, added %d changesets" +
                           " and %d new revisions\n")
                           % (files, changesets, revisions))
            tr.close()
            return

        node = self.manifest.add(nmap, tr, resolverev, mm, mo)
        revisions += 1

        # Now all files and manifests are merged, we add the changed files
        # and manifest id to the changelog
        self.ui.status("committing merge changeset\n")
        if co == cn: cn = -1

        edittext = "\nHG: merge resolve\n" + \
                   "HG: manifest hash %s\n" % hex(node) + \
                   "".join(["HG: changed %s\n" % f for f in new])
        edittext = self.ui.edit(edittext)
        n = self.changelog.add(node, new, edittext, tr, co, cn)
        revisions += 1

        self.ui.status("added %d changesets, %d files, and %d new revisions\n"
                       % (changesets, files, revisions))

        tr.close()

    def merge3(self, fl, fn, my, other, transaction, link):
        """perform a 3-way merge and append the result"""
        
        def temp(prefix, node):
            pre = "%s~%s." % (os.path.basename(fn), prefix)
            (fd, name) = tempfile.mkstemp("", pre)
            f = os.fdopen(fd, "w")
            f.write(fl.revision(node))
            f.close()
            return name

        base = fl.ancestor(my, other)
        self.ui.note("resolving %s\n" % fn)
        self.ui.debug("local %s remote %s ancestor %s\n" %
                              (short(my), short(other), short(base)))

        if my == base: 
            text = fl.revision(other)
        else:
            a = temp("local", my)
            b = temp("remote", other)
            c = temp("parent", base)

            cmd = os.environ["HGMERGE"]
            self.ui.debug("invoking merge with %s\n" % cmd)
            r = os.system("%s %s %s %s %s" % (cmd, a, b, c, fn))
            if r:
                raise "Merge failed!"

            text = open(a).read()
            os.unlink(a)
            os.unlink(b)
            os.unlink(c)
            
        return fl.add(text, transaction, link, my, other)

class remoterepository:
    def __init__(self, ui, path):
        self.url = path
        self.ui = ui

    def do_cmd(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        q = {"cmd": cmd}
        q.update(args)
        qs = urllib.urlencode(q)
        cu = "%s?%s" % (self.url, qs)
        return urllib.urlopen(cu)

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.do_cmd("branches", nodes=n).read()
        br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
        return br

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.do_cmd("between", pairs=n).read()
        p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
        return p

    def changegroup(self, nodes):
        n = " ".join(map(hex, nodes))
        zd = zlib.decompressobj()
        f = self.do_cmd("changegroup", roots=n)
        bytes = 0
        while 1:
            d = f.read(4096)
            bytes += len(d)
            if not d:
                yield zd.flush()
                break
            yield zd.decompress(d)
        self.ui.note("%d bytes of data transfered\n" % bytes)

def repository(ui, path=None, create=0):
    if path and path[:7] == "http://":
        return remoterepository(ui, path)
    if path and path[:5] == "hg://":
        return remoterepository(ui, path.replace("hg://", "http://"))
    if path and path[:11] == "old-http://":
        return localrepository(ui, path.replace("old-http://", "http://"))
    else:
        return localrepository(ui, path, create)

class ui:
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True):
        self.quiet = quiet and not verbose and not debug
        self.verbose = verbose or debug
        self.debugflag = debug
        self.interactive = interactive
    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))
    def readline(self):
        return sys.stdin.readline()[:-1]
    def prompt(self, msg, pat, default = "y"):
        if not self.interactive: return default
        while 1:
            self.write(msg, " ")
            r = self.readline()
            if re.match(pat, r):
                return r
            else:
                self.write("unrecognized response\n")
    def status(self, *msg):
        if not self.quiet: self.write(*msg)
    def warn(self, msg):
        self.write(*msg)
    def note(self, *msg):
        if self.verbose: self.write(*msg)
    def debug(self, *msg):
        if self.debugflag: self.write(*msg)
    def edit(self, text):
        (fd, name) = tempfile.mkstemp("hg")
        f = os.fdopen(fd, "w")
        f.write(text)
        f.close()

        editor = os.environ.get("HGEDITOR") or os.environ.get("EDITOR", "vi")
        r = os.system("%s %s" % (editor, name))

        if r:
            raise "Edit failed!"

        t = open(name).read()
        t = re.sub("(?m)^HG:.*\n", "", t)

        return t
    
class httprangereader:
    def __init__(self, url):
        self.url = url
        self.pos = 0
    def seek(self, pos):
        self.pos = pos
    def read(self, bytes=None):
        opener = urllib2.build_opener(byterange.HTTPRangeHandler())
        urllib2.install_opener(opener)
        req = urllib2.Request(self.url)
        end = ''
        if bytes: end = self.pos + bytes
        req.add_header('Range', 'bytes=%d-%s' % (self.pos, end))
        f = urllib2.urlopen(req)
        return f.read()
