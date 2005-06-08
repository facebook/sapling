# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import sys, struct, os
from revlog import *
from demandload import *
demandload(globals(), "re lock urllib urllib2 transaction time socket")
demandload(globals(), "tempfile byterange difflib")

def is_exec(f):
    return (os.stat(f).st_mode & 0100 != 0)

def set_exec(f, mode):
    s = os.stat(f).st_mode
    if (s & 0100 != 0) == mode:
        return
    os.chmod(f, s & 0666 | (mode * 0111))

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
            sm = difflib.SequenceMatcher(None, strip(parent), strip(child))
            for o, m, n, s, t in sm.get_opcodes():
                if o == 'equal':
                    new += parent[m:n]
                else:
                    new += child[s:t]
            return new

        # find all ancestors
        needed = {node:1}
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
        flag = {}
        self.listcache = (text, text.splitlines(1))
        for l in self.listcache[1]:
            (f, n) = l.split('\0')
            map[f] = bin(n[:40])
            flag[f] = (n[40:-1] == "x")
        self.mapcache = (node, map, flag)
        return map

    def readflags(self, node):
        if self.mapcache or self.mapcache[0] != node:
            self.read(node)
        return self.mapcache[2]

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

    def add(self, map, flags, transaction, link, p1=None, p2=None):
        files = map.keys()
        files.sort()

        self.addlist = ["%s\000%s%s\n" %
                        (f, hex(map[f]), flags[f] and "x" or '')
                        for f in files]
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

class dirstate:
    def __init__(self, opener, ui, root):
        self.opener = opener
        self.root = root
        self.dirty = 0
        self.ui = ui
        self.map = None
        self.pl = None

    def __del__(self):
        if self.dirty:
            self.write()

    def __getitem__(self, key):
        try:
            return self.map[key]
        except TypeError:
            self.read()
            return self[key]

    def __contains__(self, key):
        if not self.map: self.read()
        return key in self.map

    def parents(self):
        if not self.pl:
            self.read()
        return self.pl

    def setparents(self, p1, p2 = nullid):
        self.dirty = 1
        self.pl = p1, p2

    def state(self, key):
        try:
            return self[key][0]
        except KeyError:
            return "?"

    def read(self):
        if self.map is not None: return self.map

        self.map = {}
        self.pl = [nullid, nullid]
        try:
            st = self.opener("dirstate").read()
        except: return

        self.pl = [st[:20], st[20: 40]]

        pos = 40
        while pos < len(st):
            e = struct.unpack(">cllll", st[pos:pos+17])
            l = e[4]
            pos += 17
            f = st[pos:pos + l]
            self.map[f] = e[:4]
            pos += l
        
    def update(self, files, state):
        ''' current states:
        n  normal
        m  needs merging
        r  marked for removal
        a  marked for addition'''

        if not files: return
        self.read()
        self.dirty = 1
        for f in files:
            if state == "r":
                self.map[f] = ('r', 0, 0, 0)
            else:
                s = os.stat(os.path.join(self.root, f))
                self.map[f] = (state, s.st_mode, s.st_size, s.st_mtime)

    def forget(self, files):
        if not files: return
        self.read()
        self.dirty = 1
        for f in files:
            try:
                del self.map[f]
            except KeyError:
                self.ui.warn("not in dirstate: %s!\n" % f)
                pass

    def clear(self):
        self.map = {}
        self.dirty = 1

    def write(self):
        st = self.opener("dirstate", "w")
        st.write("".join(self.pl))
        for f, e in self.map.items():
            e = struct.pack(">cllll", e[0], e[1], e[2], e[3], len(f))
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
            self.dirstate = dirstate(self.opener, ui, self.root)

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
                # read each head of the tags file, ending with the tip
                # and add each tag found to the map, with "newer" ones
                # taking precedence
                fl = self.file(".hgtags")
                h = fl.heads()
                h.reverse()
                for r in h:
                    for l in fl.revision(r).splitlines():
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

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/': f = f[1:]
        return filelog(self.opener, f)

    def transaction(self):
        # save dirstate for undo
        try:
            ds = self.opener("dirstate").read()
        except IOError:
            ds = ""
        self.opener("undo.dirstate", "w").write(ds)
        
        return transaction.transaction(self.opener, self.join("journal"),
                                       self.join("undo"))

    def recover(self):
        lock = self.lock()
        if os.path.exists(self.join("recover")):
            self.ui.status("attempting to rollback interrupted transaction\n")
            return transaction.rollback(self.opener, self.join("recover"))
        else:
            self.ui.warn("no interrupted transaction available\n")

    def undo(self):
        lock = self.lock()
        if os.path.exists(self.join("undo")):
            self.ui.status("attempting to rollback last transaction\n")
            transaction.rollback(self.opener, self.join("undo"))
            self.dirstate = None
            os.rename(self.join("undo.dirstate"), self.join("dirstate"))
            self.dirstate = dirstate(self.opener, self.ui, self.root)
        else:
            self.ui.warn("no undo information available\n")

    def lock(self, wait = 1):
        try:
            return lock.lock(self.join("lock"), 0)
        except lock.LockHeld, inst:
            if wait:
                self.ui.warn("waiting for lock held by %s\n" % inst.args[0])
                return lock.lock(self.join("lock"), wait)
            raise inst

    def rawcommit(self, files, text, user, date, p1=None, p2=None):
        p1 = p1 or self.dirstate.parents()[0] or nullid
        p2 = p2 or self.dirstate.parents()[1] or nullid
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
            # FIXME - need to find both parents properly
            prev = pmmap.get(f, nullid)
            mmap[f] = r.add(t, tr, linkrev, prev)

        mnode = self.manifest.add(mmap, tr, linkrev, pchange[0])
        n = self.changelog.add(mnode, files, text, tr, p1, p2, user ,date, )
        tr.close()
        self.dirstate.setparents(p1, p2)
        self.dirstate.clear()
        self.dirstate.update(mmap.keys(), "n")

    def commit(self, files = None, text = ""):
        commit = []
        remove = []
        if files:
            for f in files:
                s = self.dirstate.state(f)
                if s in 'nmai':
                    commit.append(f)
                elif s == 'r':
                    remove.append(f)
                else:
                    self.ui.warn("%s not tracked!\n" % f)
        else:
            (c, a, d, u) = self.diffdir(self.root)
            commit = c + a
            remove = d

        if not commit and not remove:
            self.ui.status("nothing changed\n")
            return

        p1, p2 = self.dirstate.parents()
        c1 = self.changelog.read(p1)
        c2 = self.changelog.read(p2)
        m1 = self.manifest.read(c1[0])
        mf1 = self.manifest.readflags(c1[0])
        m2 = self.manifest.read(c2[0])
        lock = self.lock()
        tr = self.transaction()

        # check in files
        new = {}
        linkrev = self.changelog.count()
        commit.sort()
        for f in commit:
            self.ui.note(f + "\n")
            try:
                fp = self.wjoin(f)
                mf1[f] = is_exec(fp)
                t = file(fp).read()
            except IOError:
                self.warn("trouble committing %s!\n" % f)
                raise

            r = self.file(f)
            fp1 = m1.get(f, nullid)
            fp2 = m2.get(f, nullid)
            new[f] = r.add(t, tr, linkrev, fp1, fp2)

        # update manifest
        m1.update(new)
        for f in remove: del m1[f]
        mn = self.manifest.add(m1, mf1, tr, linkrev, c1[0], c2[0])

        # add changeset
        new = new.keys()
        new.sort()

        edittext = text + "\n" + "HG: manifest hash %s\n" % hex(mn)
        edittext += "".join(["HG: changed %s\n" % f for f in new])
        edittext += "".join(["HG: removed %s\n" % f for f in remove])
        edittext = self.ui.edit(edittext)

        n = self.changelog.add(mn, new, edittext, tr, p1, p2)
        tr.close()

        self.dirstate.setparents(n)
        self.dirstate.update(new, "n")
        self.dirstate.forget(remove)

    def diffdir(self, path, changeset = None):
        changed = []
        added = []
        unknown = []
        mf = {}

        if changeset:
            change = self.changelog.read(changeset)
            mf = self.manifest.read(change[0])
            dc = dict.fromkeys(mf)
        else:
            changeset = self.dirstate.parents()[0]
            change = self.changelog.read(changeset)
            mf = self.manifest.read(change[0])
            dc = self.dirstate.copy()

        def fcmp(fn):
            t1 = file(self.wjoin(fn)).read()
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
                    elif c[0] == 'm':
                        changed.append(fn)
                    elif c[0] == 'a':
                        added.append(fn)
                    elif c[0] == 'r':
                        unknown.append(fn)
                    elif c[2] != s.st_size or (c[1] ^ s.st_mode) & 0100:
                        changed.append(fn)
                    elif c[1] != s.st_mode or c[3] != s.st_mtime:
                        if fcmp(fn):
                            changed.append(fn)
                else:
                    if self.ignore(fn): continue
                    unknown.append(fn)

        deleted = dc.keys()
        deleted.sort()

        return (changed, added, deleted, unknown)

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
        for f in list:
            p = self.wjoin(f)
            if not os.path.isfile(p):
                self.ui.warn("%s does not exist!\n" % f)
            elif self.dirstate.state(f) == 'n':
                self.ui.warn("%s already tracked!\n" % f)
            else:
                self.dirstate.update([f], "a")

    def forget(self, list):
        for f in list:
            if self.dirstate.state(f) not in 'ai':
                self.ui.warn("%s not added!\n" % f)
            else:
                self.dirstate.forget([f])

    def remove(self, list):
        for f in list:
            p = self.wjoin(f)
            if os.path.isfile(p):
                self.ui.warn("%s still exists!\n" % f)
            elif f not in self.dirstate:
                self.ui.warn("%s not tracked!\n" % f)
            else:
                self.dirstate.update([f], "r")

    def heads(self):
        return self.changelog.heads()

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

        # if we have an empty repo, fetch everything
        if self.changelog.tip() == nullid:
            self.ui.status("requesting all changes\n")
            return remote.changegroup([nullid])

        # otherwise, assume we're closer to the tip than the root
        self.ui.status("searching for changes\n")
        heads = remote.heads()
        unknown = []
        for h in heads:
            if h not in m:
                unknown.append(h)

        if not unknown:
            self.ui.status("nothing to do!\n")
            return None
            
        unknown = remote.branches(unknown)
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

        def csmap(x):
            self.ui.debug("add changeset %s\n" % short(x))
            return self.changelog.count()

        def revmap(x):
            return self.changelog.rev(x)

        if not generator: return
        changesets = files = revisions = 0

        source = genread(generator)
        lock = self.lock()
        tr = self.transaction()

        # pull off the changeset group
        self.ui.status("adding changesets\n")
        co = self.changelog.tip()
        cn = self.changelog.addgroup(getgroup(), csmap, tr, 1) # unique
        changesets = self.changelog.rev(cn) - self.changelog.rev(co)

        # pull off the manifest group
        self.ui.status("adding manifests\n")
        mm = self.manifest.tip()
        mo = self.manifest.addgroup(getgroup(), revmap, tr)

        # process the files
        self.ui.status("adding file revisions\n")
        while 1:
            f = getchunk()
            if not f: break
            self.ui.debug("adding %s revisions\n" % f)
            fl = self.file(f)
            o = fl.tip()
            n = fl.addgroup(getgroup(), revmap, tr)
            revisions += fl.rev(n) - fl.rev(o)
            files += 1

        self.ui.status(("modified %d files, added %d changesets" +
                        " and %d new revisions\n")
                       % (files, changesets, revisions))

        tr.close()
        return

    def update(self, node, allow=False, force=False):
        pl = self.dirstate.parents()
        if not force and pl[1] != nullid:
            self.ui.warn("aborting: outstanding uncommitted merges\n")
            return

        p1, p2 = pl[0], node
        m1n = self.changelog.read(p1)[0]
        m2n = self.changelog.read(p2)[0]
        man = self.manifest.ancestor(m1n, m2n)
        m1 = self.manifest.read(m1n)
        mf1 = self.manifest.readflags(m1n)
        m2 = self.manifest.read(m2n)
        mf2 = self.manifest.readflags(m2n)
        ma = self.manifest.read(man)
        mfa = self.manifest.readflags(m2n)

        (c, a, d, u) = self.diffdir(self.root)

        # resolve the manifest to determine which files
        # we care about merging
        self.ui.note("resolving manifests\n")
        self.ui.debug(" ancestor %s local %s remote %s\n" %
                      (short(man), short(m1n), short(m2n)))

        merge = {}
        get = {}
        remove = []

        # construct a working dir manifest
        mw = m1.copy()
        mfw = mf1.copy()
        for f in a + c + u:
            mw[f] = ""
            mfw[f] = is_exec(self.wjoin(f))
        for f in d:
            if f in mw: del mw[f]

        for f, n in mw.iteritems():
            if f in m2:
                s = 0

                if n != m2[f]:
                    a = ma.get(f, nullid)
                    if n != a and m2[f] != a:
                        self.ui.debug(" %s versions differ, resolve\n" % f)
                        merge[f] = (m1.get(f, nullid), m2[f])
                        # merge executable bits
                        # "if we changed or they changed, change in merge"
                        a, b, c = mfa.get(f, 0), mfw[f], mf2[f]
                        mode = ((a^b) | (a^c)) ^ a
                        merge[f] = (m1.get(f, nullid), m2[f], mode)
                        s = 1
                    elif m2[f] != a:
                        self.ui.debug(" remote %s is newer, get\n" % f)
                        get[f] = m2[f]
                        s = 1

                if not s and mfw[f] != mf2[f]:
                    if force:
                        self.ui.debug(" updating permissions for %s\n" % f)
                        set_exec(self.wjoin(f), mf2[f])
                    else:
                        a, b, c = mfa.get(f, 0), mfw[f], mf2[f]
                        mode = ((a^b) | (a^c)) ^ a
                        if mode != b:
                            self.ui.debug(" updating permissions for %s\n" % f)
                            set_exec(self.wjoin(f), mode)

                del m2[f]
            elif f in ma:
                if not force and n != ma[f]:
                    r = self.ui.prompt(
                        (" local changed %s which remote deleted\n" % f) +
                        "(k)eep or (d)elete?", "[kd]", "k")
                    if r == "d":
                        remove.append(f)
                else:
                    self.ui.debug("other deleted %s\n" % f)
                    remove.append(f) # other deleted it
            else:
                if n == m1.get(f, nullid): # same as parent
                    self.ui.debug("remote deleted %s\n" % f)
                    remove.append(f)
                else:
                    self.ui.debug("working dir created %s, keeping\n" % f)

        for f, n in m2.iteritems():
            if f[0] == "/": continue
            if not force and f in ma and n != ma[f]:
                r = self.ui.prompt(
                    ("remote changed %s which local deleted\n" % f) +
                    "(k)eep or (d)elete?", "[kd]", "k")
                if r == "d": remove.append(f)
            else:
                self.ui.debug("remote created %s\n" % f)
                get[f] = n

        del mw, m1, m2, ma

        if force:
            for f in merge:
                get[f] = merge[f][1]
            merge = {}

        if not merge:
            # we don't need to do any magic, just jump to the new rev
            mode = 'n'
            p1, p2 = p2, nullid
        else:
            if not allow:
                self.ui.status("the following files conflict:\n")
                for f in merge:
                    self.ui.status(" %s\n" % f)
                self.ui.warn("aborting update due to conflicting files!\n")
                self.ui.status("(use update -m to allow a merge)\n")
                return 1
            # we have to remember what files we needed to get/change
            # because any file that's different from either one of its
            # parents must be in the changeset
            mode = 'm'

        self.dirstate.setparents(p1, p2)

        # get the files we don't need to change
        files = get.keys()
        files.sort()
        for f in files:
            if f[0] == "/": continue
            self.ui.note("getting %s\n" % f)
            t = self.file(f).read(get[f])
            wp = self.wjoin(f)
            try:
                file(wp, "w").write(t)
            except IOError:
                os.makedirs(os.path.dirname(wp))
                file(wp, "w").write(t)
            set_exec(wp, mf2[f])
            self.dirstate.update([f], mode)

        # merge the tricky bits
        files = merge.keys()
        files.sort()
        for f in files:
            self.ui.status("merging %s\n" % f)
            m, o, flag = merge[f]
            self.merge3(f, m, o)
            set_exec(wp, flag)
            self.dirstate.update([f], 'm')

        for f in remove:
            self.ui.note("removing %s\n" % f)
            os.unlink(f)
        if mode == 'n':
            self.dirstate.forget(remove)
        else:
            self.dirstate.update(remove, 'r')

    def merge3(self, fn, my, other):
        """perform a 3-way merge in the working directory"""

        def temp(prefix, node):
            pre = "%s~%s." % (os.path.basename(fn), prefix)
            (fd, name) = tempfile.mkstemp("", pre)
            f = os.fdopen(fd, "w")
            f.write(fl.revision(node))
            f.close()
            return name

        fl = self.file(fn)
        base = fl.ancestor(my, other)
        a = self.wjoin(fn)
        b = temp("other", other)
        c = temp("base", base)

        self.ui.note("resolving %s\n" % fn)
        self.ui.debug("file %s: other %s ancestor %s\n" %
                              (fn, short(other), short(base)))

        cmd = os.environ.get("HGMERGE", "hgmerge")
        r = os.system("%s %s %s %s" % (cmd, a, b, c))
        if r:
            self.ui.warn("merging %s failed!\n" % fn)

        os.unlink(b)
        os.unlink(c)

    def verify(self):
        filelinkrevs = {}
        filenodes = {}
        manifestchangeset = {}
        changesets = revisions = files = 0
        errors = 0

        self.ui.status("checking changesets\n")
        for i in range(self.changelog.count()):
            changesets += 1
            n = self.changelog.node(i)
            for p in self.changelog.parents(n):
                if p not in self.changelog.nodemap:
                    self.ui.warn("changeset %s has unknown parent %s\n" %
                                 (short(n), short(p)))
                    errors += 1
            try:
                changes = self.changelog.read(n)
            except Exception, inst:
                self.ui.warn("unpacking changeset %s: %s\n" % (short(n), inst))
                errors += 1

            manifestchangeset[changes[0]] = n
            for f in changes[3]:
                filelinkrevs.setdefault(f, []).append(i)

        self.ui.status("checking manifests\n")
        for i in range(self.manifest.count()):
            n = self.manifest.node(i)
            for p in self.manifest.parents(n):
                if p not in self.manifest.nodemap:
                    self.ui.warn("manifest %s has unknown parent %s\n" %
                            (short(n), short(p)))
                    errors += 1
            ca = self.changelog.node(self.manifest.linkrev(n))
            cc = manifestchangeset[n]
            if ca != cc:
                self.ui.warn("manifest %s points to %s, not %s\n" %
                        (hex(n), hex(ca), hex(cc)))
                errors += 1

            try:
                delta = mdiff.patchtext(self.manifest.delta(n))
            except KeyboardInterrupt:
                print "aborted"
                sys.exit(0)
            except Exception, inst:
                self.ui.warn("unpacking manifest %s: %s\n"
                             % (short(n), inst))
                errors += 1

            ff = [ l.split('\0') for l in delta.splitlines() ]
            for f, fn in ff:
                filenodes.setdefault(f, {})[bin(fn)] = 1

        self.ui.status("crosschecking files in changesets and manifests\n")
        for f in filenodes:
            if f not in filelinkrevs:
                self.ui.warn("file %s in manifest but not in changesets\n" % f)
                errors += 1

        for f in filelinkrevs:
            if f not in filenodes:
                self.ui.warn("file %s in changeset but not in manifest\n" % f)
                errors += 1

        self.ui.status("checking files\n")
        ff = filenodes.keys()
        ff.sort()
        for f in ff:
            if f == "/dev/null": continue
            files += 1
            fl = self.file(f)
            nodes = { nullid: 1 }
            for i in range(fl.count()):
                revisions += 1
                n = fl.node(i)

                if n not in filenodes[f]:
                    self.ui.warn("%s: %d:%s not in manifests\n"
                                 % (f, i, short(n)))
                    print len(filenodes[f].keys()), fl.count(), f
                    errors += 1
                else:
                    del filenodes[f][n]

                flr = fl.linkrev(n)
                if flr not in filelinkrevs[f]:
                    self.ui.warn("%s:%s points to unexpected changeset %d\n"
                            % (f, short(n), fl.linkrev(n)))
                    errors += 1
                else:
                    filelinkrevs[f].remove(flr)

                # verify contents
                try:
                    t = fl.read(n)
                except Exception, inst:
                    self.ui.warn("unpacking file %s %s: %s\n"
                                 % (f, short(n), inst))
                    errors += 1

                # verify parents
                (p1, p2) = fl.parents(n)
                if p1 not in nodes:
                    self.ui.warn("file %s:%s unknown parent 1 %s" %
                            (f, short(n), short(p1)))
                    errors += 1
                if p2 not in nodes:
                    self.ui.warn("file %s:%s unknown parent 2 %s" %
                            (f, short(n), short(p1)))
                    errors += 1
                nodes[n] = 1

            # cross-check
            for node in filenodes[f]:
                self.ui.warn("node %s in manifests not in %s\n"
                             % (hex(n), f))
                errors += 1

        self.ui.status("%d files, %d changesets, %d total revisions\n" %
                       (files, changesets, revisions))

        if errors:
            self.ui.warn("%d integrity errors encountered!\n" % errors)
            return 1

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

    def heads(self):
        d = self.do_cmd("heads").read()
        try:
            return map(bin, d[:-1].split(" "))
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.do_cmd("branches", nodes=n).read()
        try:
            br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
            return br
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.do_cmd("between", pairs=n).read()
        try:
            p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
            return p
        except:
            self.ui.warn("unexpected response:\n" + d[:400] + "\n...\n")
            raise

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
