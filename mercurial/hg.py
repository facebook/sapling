# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import sys, struct, os
import util
from revlog import *
from demandload import *
demandload(globals(), "re lock urllib urllib2 transaction time socket")
demandload(globals(), "tempfile httprangereader bdiff urlparse")
demandload(globals(), "bisect select")

class filelog(revlog):
    def __init__(self, opener, path):
        revlog.__init__(self, opener,
                        os.path.join("data", self.encodedir(path + ".i")),
                        os.path.join("data", self.encodedir(path + ".d")))

    # This avoids a collision between a file named foo and a dir named
    # foo.i or foo.d
    def encodedir(self, path):
        path.replace(".hg/", ".hg.hg/")
        path.replace(".i/", ".i.hg/")
        path.replace(".d/", ".i.hg/")
        return path

    def decodedir(self, path):
        path.replace(".d.hg/", ".d/")
        path.replace(".i.hg/", ".i/")
        path.replace(".hg.hg/", ".hg/")
        return path

    def read(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.find('\1\n', 2)
        return t[s+2:]

    def readmeta(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.find('\1\n', 2)
        mt = t[2:s]
        for l in mt.splitlines():
            k, v = l.split(": ", 1)
            m[k] = v
        return m

    def add(self, text, meta, transaction, link, p1=None, p2=None):
        if meta or text.startswith('\1\n'):
            mt = ""
            if meta:
                mt = [ "%s: %s\n" % (k, v) for k,v in meta.items() ]
            text = "\1\n" + "".join(mt) + "\1\n" + text
        return self.addrevision(text, transaction, link, p1, p2)

    def annotate(self, node):

        def decorate(text, rev):
            return ([rev] * len(text.splitlines()), text)

        def pair(parent, child):
            for a1, a2, b1, b2 in bdiff.blocks(parent[1], child[1]):
                child[0][b1:b2] = parent[0][a1:a2]
            return child

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
        visit = [ (self.rev(n), n) for n in needed.keys() ]
        visit.sort()
        hist = {}

        for r,n in visit:
            curr = decorate(self.read(n), self.linkrev(n))
            for p in self.parents(n):
                if p != nullid:
                    curr = pair(hist[p], curr)
                    # trim the history of unneeded revs
                    needed[p] -= 1
                    if not needed[p]:
                        del hist[p]
            hist[n] = curr

        return zip(hist[n][0], hist[n][1].splitlines(1))

class manifest(revlog):
    def __init__(self, opener):
        self.mapcache = None
        self.listcache = None
        self.addlist = None
        revlog.__init__(self, opener, "00manifest.i", "00manifest.d")

    def read(self, node):
        if node == nullid: return {} # don't upset local cache
        if self.mapcache and self.mapcache[0] == node:
            return self.mapcache[1]
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
        if node == nullid: return {} # don't upset local cache
        if not self.mapcache or self.mapcache[0] != node:
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

    def add(self, map, flags, transaction, link, p1=None, p2=None,
            changed=None):
        # directly generate the mdiff delta from the data collected during
        # the bisect loop below
        def gendelta(delta):
            i = 0
            result = []
            while i < len(delta):
                start = delta[i][2]
                end = delta[i][3]
                l = delta[i][4]
                if l == None:
                    l = ""
                while i < len(delta) - 1 and start <= delta[i+1][2] \
                          and end >= delta[i+1][2]:
                    if delta[i+1][3] > end:
                        end = delta[i+1][3]
                    if delta[i+1][4]:
                        l += delta[i+1][4]
                    i += 1
                result.append(struct.pack(">lll", start, end, len(l)) +  l)
                i += 1
            return result

        # apply the changes collected during the bisect loop to our addlist
        def addlistdelta(addlist, delta):
            # apply the deltas to the addlist.  start from the bottom up
            # so changes to the offsets don't mess things up.
            i = len(delta)
            while i > 0:
                i -= 1
                start = delta[i][0]
                end = delta[i][1]
                if delta[i][4]:
                    addlist[start:end] = [delta[i][4]]
                else:
                    del addlist[start:end]
            return addlist

        # calculate the byte offset of the start of each line in the
        # manifest
        def calcoffsets(addlist):
            offsets = [0] * (len(addlist) + 1)
            offset = 0
            i = 0
            while i < len(addlist):
                offsets[i] = offset
                offset += len(addlist[i])
                i += 1
            offsets[i] = offset
            return offsets

        # if we're using the listcache, make sure it is valid and
        # parented by the same node we're diffing against
        if not changed or not self.listcache or not p1 or \
               self.mapcache[0] != p1:
            files = map.keys()
            files.sort()

            self.addlist = ["%s\000%s%s\n" %
                            (f, hex(map[f]), flags[f] and "x" or '')
                            for f in files]
            cachedelta = None
        else:
            addlist = self.listcache[1]

            # find the starting offset for each line in the add list
            offsets = calcoffsets(addlist)

            # combine the changed lists into one list for sorting
            work = [[x, 0] for x in changed[0]]
            work[len(work):] = [[x, 1] for x in changed[1]]
            work.sort()

            delta = []
            bs = 0

            for w in work:
                f = w[0]
                # bs will either be the index of the item or the insert point
                bs = bisect.bisect(addlist, f, bs)
                if bs < len(addlist):
                    fn = addlist[bs][:addlist[bs].index('\0')]
                else:
                    fn = None
                if w[1] == 0:
                    l = "%s\000%s%s\n" % (f, hex(map[f]),
                                          flags[f] and "x" or '')
                else:
                    l = None
                start = bs
                if fn != f:
                    # item not found, insert a new one
                    end = bs
                    if w[1] == 1:
                        sys.stderr.write("failed to remove %s from manifest\n"
                                         % f)
                        sys.exit(1)
                else:
                    # item is found, replace/delete the existing line
                    end = bs + 1
                delta.append([start, end, offsets[start], offsets[end], l])

            self.addlist = addlistdelta(addlist, delta)
            if self.mapcache[0] == self.tip():
                cachedelta = "".join(gendelta(delta))
            else:
                cachedelta = None

        text = "".join(self.addlist)
        if cachedelta and mdiff.patch(self.listcache[0], cachedelta) != text:
            sys.stderr.write("manifest delta failure\n")
            sys.exit(1)
        n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        self.mapcache = (n, map, flags)
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
        self.copies = {}
        self.ignorefunc = None

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def ignore(self, f):
        if not self.ignorefunc:
            bigpat = []
            try:
                l = file(self.wjoin(".hgignore"))
                for pat in l:
                    if pat != "\n":
                        p = util.pconvert(pat[:-1])
                        try:
                            r = re.compile(p)
                        except:
                            self.ui.warn("ignoring invalid ignore"
                                         + " regular expression '%s'\n" % p)
                        else:
                            bigpat.append(util.pconvert(pat[:-1]))
            except IOError: pass

            if bigpat:
                s = "(?:%s)" % (")|(?:".join(bigpat))
                r = re.compile(s)
                self.ignorefunc = r.search
            else:
                self.ignorefunc = util.never

        return self.ignorefunc(f)

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

    def markdirty(self):
        if not self.dirty:
            self.dirty = 1

    def setparents(self, p1, p2 = nullid):
        self.markdirty()
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
            if not st: return
        except: return

        self.pl = [st[:20], st[20: 40]]

        pos = 40
        while pos < len(st):
            e = struct.unpack(">cllll", st[pos:pos+17])
            l = e[4]
            pos += 17
            f = st[pos:pos + l]
            if '\0' in f:
                f, c = f.split('\0')
                self.copies[f] = c
            self.map[f] = e[:4]
            pos += l

    def copy(self, source, dest):
        self.read()
        self.markdirty()
        self.copies[dest] = source

    def copied(self, file):
        return self.copies.get(file, None)

    def update(self, files, state):
        ''' current states:
        n  normal
        m  needs merging
        r  marked for removal
        a  marked for addition'''

        if not files: return
        self.read()
        self.markdirty()
        for f in files:
            if state == "r":
                self.map[f] = ('r', 0, 0, 0)
            else:
                s = os.stat(os.path.join(self.root, f))
                self.map[f] = (state, s.st_mode, s.st_size, s.st_mtime)

    def forget(self, files):
        if not files: return
        self.read()
        self.markdirty()
        for f in files:
            try:
                del self.map[f]
            except KeyError:
                self.ui.warn("not in dirstate: %s!\n" % f)
                pass

    def clear(self):
        self.map = {}
        self.markdirty()

    def write(self):
        st = self.opener("dirstate", "w")
        st.write("".join(self.pl))
        for f, e in self.map.items():
            c = self.copied(f)
            if c:
                f = f + "\0" + c
            e = struct.pack(">cllll", e[0], e[1], e[2], e[3], len(f))
            st.write(e + f)
        self.dirty = 0

    def walk(self, files = None, match = util.always):
        self.read()
        dc = self.map.copy()
        # walk all files by default
        if not files: files = [self.root]
        def traverse():
            for f in util.unique(files):
                f = os.path.join(self.root, f)
                if os.path.isdir(f):
                    for dir, subdirs, fl in os.walk(f):
                        d = dir[len(self.root) + 1:]
                        if d == '.hg':
                            subdirs[:] = []
                            continue
                        for sd in subdirs:
                            ds = os.path.join(d, sd +'/')
                            if self.ignore(ds) or not match(ds):
                                subdirs.remove(sd)
                        for fn in fl:
                            fn = util.pconvert(os.path.join(d, fn))
                            yield 'f', fn
                else:
                    yield 'f', f[len(self.root) + 1:]

            for k in dc.keys():
                yield 'm', k

        # yield only files that match: all in dirstate, others only if
        # not in .hgignore

        for src, fn in util.unique(traverse()):
            if fn in dc:
                del dc[fn]
            elif self.ignore(fn):
                continue
            if match(fn):
                yield src, fn

    def changes(self, files = None, match = util.always):
        self.read()
        dc = self.map.copy()
        lookup, changed, added, unknown = [], [], [], []

        for src, fn in self.walk(files, match):
            try: s = os.stat(os.path.join(self.root, fn))
            except: continue

            if fn in dc:
                c = dc[fn]
                del dc[fn]

                if c[0] == 'm':
                    changed.append(fn)
                elif c[0] == 'a':
                    added.append(fn)
                elif c[0] == 'r':
                    unknown.append(fn)
                elif c[2] != s.st_size or (c[1] ^ s.st_mode) & 0100:
                    changed.append(fn)
                elif c[1] != s.st_mode or c[3] != s.st_mtime:
                    lookup.append(fn)
            else:
                if match(fn): unknown.append(fn)

        return (lookup, changed, added, filter(match, dc.keys()), unknown)

# used to avoid circular references so destructors work
def opener(base):
    p = base
    def o(path, mode="r"):
        if p.startswith("http://"):
            f = os.path.join(p, urllib.quote(path))
            return httprangereader.httprangereader(f)

        f = os.path.join(p, path)

        mode += "b" # for that other OS

        if mode[0] != "r":
            try:
                s = os.stat(f)
            except OSError:
                d = os.path.dirname(f)
                if not os.path.isdir(d):
                    os.makedirs(d)
            else:
                if s.st_nlink > 1:
                    file(f + ".tmp", "wb").write(file(f, "rb").read())
                    util.rename(f+".tmp", f)

        return file(f, mode)

    return o

class RepoError(Exception): pass

class localrepository:
    def __init__(self, ui, path=None, create=0):
        self.remote = 0
        if path and path.startswith("http://"):
            self.remote = 1
            self.path = path
        else:
            if not path:
                p = os.getcwd()
                while not os.path.isdir(os.path.join(p, ".hg")):
                    oldp = p
                    p = os.path.dirname(p)
                    if p == oldp: raise RepoError("no repo found")
                path = p
            self.path = os.path.join(path, ".hg")

            if not create and not os.path.isdir(self.path):
                raise RepoError("repository %s not found" % self.path)

        self.root = path
        self.ui = ui

        if create:
            os.mkdir(self.path)
            os.mkdir(self.join("data"))

        self.opener = opener(self.path)
        self.wopener = opener(self.root)
        self.manifest = manifest(self.opener)
        self.changelog = changelog(self.opener)
        self.tagscache = None
        self.nodetagscache = None

        if not self.remote:
            self.dirstate = dirstate(self.opener, ui, self.root)
            try:
                self.ui.readconfig(self.opener("hgrc"))
            except IOError: pass

    def hook(self, name, **args):
        s = self.ui.config("hooks", name)
        if s:
            self.ui.note("running hook %s: %s\n" % (name, s))
            old = {}
            for k, v in args.items():
                k = k.upper()
                old[k] = os.environ.get(k, None)
                os.environ[k] = v

            r = os.system(s)

            for k, v in old.items():
                if v != None:
                    os.environ[k] = v
                else:
                    del os.environ[k]

            if r:
                self.ui.warn("abort: %s hook failed with status %d!\n" %
                             (name, r))
                return False
        return True

    def tags(self):
        '''return a mapping of tag to node'''
        if not self.tagscache:
            self.tagscache = {}
            def addtag(self, k, n):
                try:
                    bin_n = bin(n)
                except TypeError:
                    bin_n = ''
                self.tagscache[k.strip()] = bin_n

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
                            n, k = l.split(" ", 1)
                            addtag(self, k, n)
            except KeyError:
                pass

            try:
                f = self.opener("localtags")
                for l in f:
                    n, k = l.split(" ", 1)
                    addtag(self, k, n)
            except IOError:
                pass

            self.tagscache['tip'] = self.changelog.tip()

        return self.tagscache

    def tagslist(self):
        '''return a list of tags ordered by revision'''
        l = []
        for t, n in self.tags().items():
            try:
                r = self.changelog.rev(n)
            except:
                r = -2 # sort to the beginning of the list if unknown
            l.append((r,t,n))
        l.sort()
        return [(t,n) for r,t,n in l]

    def nodetags(self, node):
        '''return the tags associated with a node'''
        if not self.nodetagscache:
            self.nodetagscache = {}
            for t,n in self.tags().items():
                self.nodetagscache.setdefault(n,[]).append(t)
        return self.nodetagscache.get(node, [])

    def lookup(self, key):
        try:
            return self.tags()[key]
        except KeyError:
            try:
                return self.changelog.lookup(key)
            except:
                raise RepoError("unknown revision '%s'" % key)

    def dev(self):
        if self.remote: return -1
        return os.stat(self.path).st_dev

    def join(self, f):
        return os.path.join(self.path, f)

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/': f = f[1:]
        return filelog(self.opener, f)

    def getcwd(self):
        cwd = os.getcwd()
        if cwd == self.root: return ''
        return cwd[len(self.root) + 1:]

    def wfile(self, f, mode='r'):
        return self.wopener(f, mode)

    def transaction(self):
        # save dirstate for undo
        try:
            ds = self.opener("dirstate").read()
        except IOError:
            ds = ""
        self.opener("journal.dirstate", "w").write(ds)

        def after():
            util.rename(self.join("journal"), self.join("undo"))
            util.rename(self.join("journal.dirstate"),
                        self.join("undo.dirstate"))

        return transaction.transaction(self.ui.warn, self.opener,
                                       self.join("journal"), after)

    def recover(self):
        lock = self.lock()
        if os.path.exists(self.join("journal")):
            self.ui.status("rolling back interrupted transaction\n")
            return transaction.rollback(self.opener, self.join("journal"))
        else:
            self.ui.warn("no interrupted transaction available\n")

    def undo(self):
        lock = self.lock()
        if os.path.exists(self.join("undo")):
            self.ui.status("rolling back last transaction\n")
            transaction.rollback(self.opener, self.join("undo"))
            self.dirstate = None
            util.rename(self.join("undo.dirstate"), self.join("dirstate"))
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
        orig_parent = self.dirstate.parents()[0] or nullid
        p1 = p1 or self.dirstate.parents()[0] or nullid
        p2 = p2 or self.dirstate.parents()[1] or nullid
        c1 = self.changelog.read(p1)
        c2 = self.changelog.read(p2)
        m1 = self.manifest.read(c1[0])
        mf1 = self.manifest.readflags(c1[0])
        m2 = self.manifest.read(c2[0])

        if orig_parent == p1:
            update_dirstate = 1
        else:
            update_dirstate = 0

        tr = self.transaction()
        mm = m1.copy()
        mfm = mf1.copy()
        linkrev = self.changelog.count()
        for f in files:
            try:
                t = self.wfile(f).read()
                tm = util.is_exec(self.wjoin(f), mfm.get(f, False))
                r = self.file(f)
                mfm[f] = tm
                mm[f] = r.add(t, {}, tr, linkrev,
                              m1.get(f, nullid), m2.get(f, nullid))
                if update_dirstate:
                    self.dirstate.update([f], "n")
            except IOError:
                try:
                    del mm[f]
                    del mfm[f]
                    if update_dirstate:
                        self.dirstate.forget([f])
                except:
                    # deleted from p2?
                    pass

        mnode = self.manifest.add(mm, mfm, tr, linkrev, c1[0], c2[0])
        user = user or self.ui.username()
        n = self.changelog.add(mnode, files, text, tr, p1, p2, user, date)
        tr.close()
        if update_dirstate:
            self.dirstate.setparents(n, nullid)

    def commit(self, files = None, text = "", user = None, date = None,
               match = util.always):
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
            (c, a, d, u) = self.changes(match = match)
            commit = c + a
            remove = d

        if not commit and not remove:
            self.ui.status("nothing changed\n")
            return

        if not self.hook("precommit"):
            return 1

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
                mf1[f] = util.is_exec(self.wjoin(f), mf1.get(f, False))
                t = self.wfile(f).read()
            except IOError:
                self.ui.warn("trouble committing %s!\n" % f)
                raise

            meta = {}
            cp = self.dirstate.copied(f)
            if cp:
                meta["copy"] = cp
                meta["copyrev"] = hex(m1.get(cp, m2.get(cp, nullid)))
                self.ui.debug(" %s: copy %s:%s\n" % (f, cp, meta["copyrev"]))

            r = self.file(f)
            fp1 = m1.get(f, nullid)
            fp2 = m2.get(f, nullid)
            new[f] = r.add(t, meta, tr, linkrev, fp1, fp2)

        # update manifest
        m1.update(new)
        for f in remove:
            if f in m1:
                del m1[f]
        mn = self.manifest.add(m1, mf1, tr, linkrev, c1[0], c2[0],
                               (new, remove))

        # add changeset
        new = new.keys()
        new.sort()

        if not text:
            edittext = "\n" + "HG: manifest hash %s\n" % hex(mn)
            edittext += "".join(["HG: changed %s\n" % f for f in new])
            edittext += "".join(["HG: removed %s\n" % f for f in remove])
            edittext = self.ui.edit(edittext)
            if not edittext.rstrip():
                return 1
            text = edittext

        user = user or self.ui.username()
        n = self.changelog.add(mn, new, text, tr, p1, p2, user, date)

        tr.close()

        self.dirstate.setparents(n)
        self.dirstate.update(new, "n")
        self.dirstate.forget(remove)

        if not self.hook("commit", node=hex(n)):
            return 1

    def walk(self, node = None, files = [], match = util.always):
        if node:
            for fn in self.manifest.read(self.changelog.read(node)[0]):
                yield 'm', fn
        else:
            for src, fn in self.dirstate.walk(files, match):
                yield src, fn

    def changes(self, node1 = None, node2 = None, files = [],
                match = util.always):
        mf2, u = None, []

        def fcmp(fn, mf):
            t1 = self.wfile(fn).read()
            t2 = self.file(fn).revision(mf[fn])
            return cmp(t1, t2)

        def mfmatches(node):
            mf = dict(self.manifest.read(node))
            for fn in mf.keys():
                if not match(fn):
                    del mf[fn]
            return mf

        # are we comparing the working directory?
        if not node2:
            l, c, a, d, u = self.dirstate.changes(files, match)

            # are we comparing working dir against its parent?
            if not node1:
                if l:
                    # do a full compare of any files that might have changed
                    change = self.changelog.read(self.dirstate.parents()[0])
                    mf2 = mfmatches(change[0])
                    for f in l:
                        if fcmp(f, mf2):
                            c.append(f)

                for l in c, a, d, u:
                    l.sort()

                return (c, a, d, u)

        # are we comparing working dir against non-tip?
        # generate a pseudo-manifest for the working dir
        if not node2:
            if not mf2:
                change = self.changelog.read(self.dirstate.parents()[0])
                mf2 = mfmatches(change[0])
            for f in a + c + l:
                mf2[f] = ""
            for f in d:
                if f in mf2: del mf2[f]
        else:
            change = self.changelog.read(node2)
            mf2 = mfmatches(change[0])

        # flush lists from dirstate before comparing manifests
        c, a = [], []

        change = self.changelog.read(node1)
        mf1 = mfmatches(change[0])

        for fn in mf2:
            if mf1.has_key(fn):
                if mf1[fn] != mf2[fn]:
                    if mf2[fn] != "" or fcmp(fn, mf1):
                        c.append(fn)
                del mf1[fn]
            else:
                a.append(fn)

        d = mf1.keys()

        for l in c, a, d, u:
            l.sort()

        return (c, a, d, u)

    def add(self, list):
        for f in list:
            p = self.wjoin(f)
            if not os.path.exists(p):
                self.ui.warn("%s does not exist!\n" % f)
            elif not os.path.isfile(p):
                self.ui.warn("%s not added: only files supported currently\n" % f)
            elif self.dirstate.state(f) in 'an':
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
            if os.path.exists(p):
                self.ui.warn("%s still exists!\n" % f)
            elif self.dirstate.state(f) == 'a':
                self.ui.warn("%s never committed!\n" % f)
                self.dirstate.forget([f])
            elif f not in self.dirstate:
                self.ui.warn("%s not tracked!\n" % f)
            else:
                self.dirstate.update([f], "r")

    def copy(self, source, dest):
        p = self.wjoin(dest)
        if not os.path.exists(p):
            self.ui.warn("%s does not exist!\n" % dest)
        elif not os.path.isfile(p):
            self.ui.warn("copy failed: %s is not a file\n" % dest)
        else:
            if self.dirstate.state(dest) == '?':
                self.dirstate.update([dest], "a")
            self.dirstate.copy(source, dest)

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

    def findincoming(self, remote, base=None, heads=None):
        m = self.changelog.nodemap
        search = []
        fetch = []
        seen = {}
        seenbranch = {}
        if base == None:
            base = {}

        # assume we're closer to the tip than the root
        # and start by examining the heads
        self.ui.status("searching for changes\n")

        if not heads:
            heads = remote.heads()

        unknown = []
        for h in heads:
            if h not in m:
                unknown.append(h)
            else:
                base[h] = 1

        if not unknown:
            return None

        rep = {}
        reqcnt = 0

        # search through remote branches
        # a 'branch' here is a linear segment of history, with four parts:
        # head, root, first parent, second parent
        # (a branch always has two parents (or none) by definition)
        unknown = remote.branches(unknown)
        while unknown:
            r = []
            while unknown:
                n = unknown.pop(0)
                if n[0] in seen:
                    continue

                self.ui.debug("examining %s:%s\n" % (short(n[0]), short(n[1])))
                if n[0] == nullid:
                    break
                if n in seenbranch:
                    self.ui.debug("branch already found\n")
                    continue
                if n[1] and n[1] in m: # do we know the base?
                    self.ui.debug("found incomplete branch %s:%s\n"
                                  % (short(n[0]), short(n[1])))
                    search.append(n) # schedule branch range for scanning
                    seenbranch[n] = 1
                else:
                    if n[1] not in seen and n[1] not in fetch:
                        if n[2] in m and n[3] in m:
                            self.ui.debug("found new changeset %s\n" %
                                          short(n[1]))
                            fetch.append(n[1]) # earliest unknown
                            base[n[2]] = 1 # latest known
                            continue

                    for a in n[2:4]:
                        if a not in rep:
                            r.append(a)
                            rep[a] = 1

                seen[n[0]] = 1

            if r:
                reqcnt += 1
                self.ui.debug("request %d: %s\n" %
                            (reqcnt, " ".join(map(short, r))))
                for p in range(0, len(r), 10):
                    for b in remote.branches(r[p:p+10]):
                        self.ui.debug("received %s:%s\n" %
                                      (short(b[0]), short(b[1])))
                        if b[0] not in m and b[0] not in seen:
                            unknown.append(b)

        # do binary search on the branches we found
        while search:
            n = search.pop(0)
            reqcnt += 1
            l = remote.between([(n[0], n[1])])[0]
            l.append(n[1])
            p = n[0]
            f = 1
            for i in l:
                self.ui.debug("narrowing %d:%d %s\n" % (f, len(l), short(i)))
                if i in m:
                    if f <= 2:
                        self.ui.debug("found new branch changeset %s\n" %
                                          short(p))
                        fetch.append(p)
                        base[i] = 1
                    else:
                        self.ui.debug("narrowed branch search to %s:%s\n"
                                      % (short(p), short(i)))
                        search.append((p, i))
                    break
                p, f = i, f * 2

        # sanity check our fetch list
        for f in fetch:
            if f in m:
                raise RepoError("already have changeset " + short(f[:4]))

        if base.keys() == [nullid]:
            self.ui.warn("warning: pulling from an unrelated repository!\n")

        self.ui.note("adding new changesets starting at " +
                     " ".join([short(f) for f in fetch]) + "\n")

        self.ui.debug("%d total queries\n" % reqcnt)

        return fetch

    def findoutgoing(self, remote, base=None, heads=None):
        if base == None:
            base = {}
            self.findincoming(remote, base, heads)

        remain = dict.fromkeys(self.changelog.nodemap)

        # prune everything remote has from the tree
        del remain[nullid]
        remove = base.keys()
        while remove:
            n = remove.pop(0)
            if n in remain:
                del remain[n]
                for p in self.changelog.parents(n):
                    remove.append(p)

        # find every node whose parents have been pruned
        subset = []
        for n in remain:
            p1, p2 = self.changelog.parents(n)
            if p1 not in remain and p2 not in remain:
                subset.append(n)

        # this is the set of all roots we have to push
        return subset

    def pull(self, remote):
        lock = self.lock()

        # if we have an empty repo, fetch everything
        if self.changelog.tip() == nullid:
            self.ui.status("requesting all changes\n")
            fetch = [nullid]
        else:
            fetch = self.findincoming(remote)

        if not fetch:
            self.ui.status("no changes found\n")
            return 1

        cg = remote.changegroup(fetch)
        return self.addchangegroup(cg)

    def push(self, remote, force=False):
        lock = remote.lock()

        base = {}
        heads = remote.heads()
        inc = self.findincoming(remote, base, heads)
        if not force and inc:
            self.ui.warn("abort: unsynced remote changes!\n")
            self.ui.status("(did you forget to sync? use push -f to force)\n")
            return 1

        update = self.findoutgoing(remote, base)
        if not update:
            self.ui.status("no changes found\n")
            return 1
        elif not force:
            if len(heads) < len(self.changelog.heads()):
                self.ui.warn("abort: push creates new remote branches!\n")
                self.ui.status("(did you forget to merge?" +
                               " use push -f to force)\n")
                return 1

        cg = self.changegroup(update)
        return remote.addchangegroup(cg)

    def changegroup(self, basenodes):
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

        def gengroup():
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

            yield struct.pack(">l", 0)

        return genread(gengroup())

    def addchangegroup(self, source):

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

        if not source: return
        changesets = files = revisions = 0

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
        self.ui.status("adding file changes\n")
        while 1:
            f = getchunk()
            if not f: break
            self.ui.debug("adding %s revisions\n" % f)
            fl = self.file(f)
            o = fl.count()
            n = fl.addgroup(getgroup(), revmap, tr)
            revisions += fl.count() - o
            files += 1

        self.ui.status(("added %d changesets" +
                        " with %d changes to %d files\n")
                       % (changesets, revisions, files))

        tr.close()

        if not self.hook("changegroup"):
            return 1

        return

    def update(self, node, allow=False, force=False, choose=None,
               moddirstate=True):
        pl = self.dirstate.parents()
        if not force and pl[1] != nullid:
            self.ui.warn("aborting: outstanding uncommitted merges\n")
            return 1

        p1, p2 = pl[0], node
        pa = self.changelog.ancestor(p1, p2)
        m1n = self.changelog.read(p1)[0]
        m2n = self.changelog.read(p2)[0]
        man = self.manifest.ancestor(m1n, m2n)
        m1 = self.manifest.read(m1n)
        mf1 = self.manifest.readflags(m1n)
        m2 = self.manifest.read(m2n)
        mf2 = self.manifest.readflags(m2n)
        ma = self.manifest.read(man)
        mfa = self.manifest.readflags(man)

        (c, a, d, u) = self.changes()

        # is this a jump, or a merge?  i.e. is there a linear path
        # from p1 to p2?
        linear_path = (pa == p1 or pa == p2)

        # resolve the manifest to determine which files
        # we care about merging
        self.ui.note("resolving manifests\n")
        self.ui.debug(" force %s allow %s moddirstate %s linear %s\n" %
                      (force, allow, moddirstate, linear_path))
        self.ui.debug(" ancestor %s local %s remote %s\n" %
                      (short(man), short(m1n), short(m2n)))

        merge = {}
        get = {}
        remove = []
        mark = {}

        # construct a working dir manifest
        mw = m1.copy()
        mfw = mf1.copy()
        umap = dict.fromkeys(u)

        for f in a + c + u:
            mw[f] = ""
            mfw[f] = util.is_exec(self.wjoin(f), mfw.get(f, False))

        for f in d:
            if f in mw: del mw[f]

            # If we're jumping between revisions (as opposed to merging),
            # and if neither the working directory nor the target rev has
            # the file, then we need to remove it from the dirstate, to
            # prevent the dirstate from listing the file when it is no
            # longer in the manifest.
            if moddirstate and linear_path and f not in m2:
                self.dirstate.forget((f,))

        # Compare manifests
        for f, n in mw.iteritems():
            if choose and not choose(f): continue
            if f in m2:
                s = 0

                # is the wfile new since m1, and match m2?
                if f not in m1:
                    t1 = self.wfile(f).read()
                    t2 = self.file(f).revision(m2[f])
                    if cmp(t1, t2) == 0:
                        mark[f] = 1
                        n = m2[f]
                    del t1, t2

                # are files different?
                if n != m2[f]:
                    a = ma.get(f, nullid)
                    # are both different from the ancestor?
                    if n != a and m2[f] != a:
                        self.ui.debug(" %s versions differ, resolve\n" % f)
                        # merge executable bits
                        # "if we changed or they changed, change in merge"
                        a, b, c = mfa.get(f, 0), mfw[f], mf2[f]
                        mode = ((a^b) | (a^c)) ^ a
                        merge[f] = (m1.get(f, nullid), m2[f], mode)
                        s = 1
                    # are we clobbering?
                    # is remote's version newer?
                    # or are we going back in time?
                    elif force or m2[f] != a or (p2 == pa and mw[f] == m1[f]):
                        self.ui.debug(" remote %s is newer, get\n" % f)
                        get[f] = m2[f]
                        s = 1
                    else:
                        mark[f] = 1
                elif f in umap:
                    # this unknown file is the same as the checkout
                    get[f] = m2[f]

                if not s and mfw[f] != mf2[f]:
                    if force:
                        self.ui.debug(" updating permissions for %s\n" % f)
                        util.set_exec(self.wjoin(f), mf2[f])
                    else:
                        a, b, c = mfa.get(f, 0), mfw[f], mf2[f]
                        mode = ((a^b) | (a^c)) ^ a
                        if mode != b:
                            self.ui.debug(" updating permissions for %s\n" % f)
                            util.set_exec(self.wjoin(f), mode)
                            mark[f] = 1
                del m2[f]
            elif f in ma:
                if n != ma[f]:
                    r = "d"
                    if not force and (linear_path or allow):
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
                    if p2 == pa: # going backwards?
                        self.ui.debug("remote deleted %s\n" % f)
                        remove.append(f)
                    else:
                        self.ui.debug("local created %s, keeping\n" % f)
                else:
                    self.ui.debug("working dir created %s, keeping\n" % f)

        for f, n in m2.iteritems():
            if choose and not choose(f): continue
            if f[0] == "/": continue
            if f in ma and n != ma[f]:
                r = "k"
                if not force and (linear_path or allow):
                    r = self.ui.prompt(
                        ("remote changed %s which local deleted\n" % f) +
                        "(k)eep or (d)elete?", "[kd]", "k")
                if r == "k": get[f] = n
            elif f not in ma:
                self.ui.debug("remote created %s\n" % f)
                get[f] = n
            else:
                if force or p2 == pa: # going backwards?
                    self.ui.debug("local deleted %s, recreating\n" % f)
                    get[f] = n
                else:
                    self.ui.debug("local deleted %s\n" % f)

        del mw, m1, m2, ma

        if force:
            for f in merge:
                get[f] = merge[f][1]
            merge = {}

        if linear_path or force:
            # we don't need to do any magic, just jump to the new rev
            mode = 'n'
            p1, p2 = p2, nullid
        else:
            if not allow:
                self.ui.status("this update spans a branch" +
                               " affecting the following files:\n")
                fl = merge.keys() + get.keys()
                fl.sort()
                for f in fl:
                    cf = ""
                    if f in merge: cf = " (resolve)"
                    self.ui.status(" %s%s\n" % (f, cf))
                self.ui.warn("aborting update spanning branches!\n")
                self.ui.status("(use update -m to merge across branches" +
                               " or -C to lose changes)\n")
                return 1
            # we have to remember what files we needed to get/change
            # because any file that's different from either one of its
            # parents must be in the changeset
            mode = 'm'
            if moddirstate:
                self.dirstate.update(mark.keys(), "m")

        if moddirstate:
            self.dirstate.setparents(p1, p2)

        # get the files we don't need to change
        files = get.keys()
        files.sort()
        for f in files:
            if f[0] == "/": continue
            self.ui.note("getting %s\n" % f)
            t = self.file(f).read(get[f])
            try:
                self.wfile(f, "w").write(t)
            except IOError:
                os.makedirs(os.path.dirname(self.wjoin(f)))
                self.wfile(f, "w").write(t)
            util.set_exec(self.wjoin(f), mf2[f])
            if moddirstate:
                self.dirstate.update([f], mode)

        # merge the tricky bits
        files = merge.keys()
        files.sort()
        for f in files:
            self.ui.status("merging %s\n" % f)
            m, o, flag = merge[f]
            self.merge3(f, m, o)
            util.set_exec(self.wjoin(f), flag)
            if moddirstate and mode == 'm':
                # only update dirstate on branch merge, otherwise we
                # could mark files with changes as unchanged
                self.dirstate.update([f], mode)

        remove.sort()
        for f in remove:
            self.ui.note("removing %s\n" % f)
            try:
                os.unlink(f)
            except OSError, inst:
                self.ui.warn("update failed to remove %s: %s!\n" % (f, inst))
            # try removing directories that might now be empty
            try: os.removedirs(os.path.dirname(f))
            except: pass
        if moddirstate:
            if mode == 'n':
                self.dirstate.forget(remove)
            else:
                self.dirstate.update(remove, 'r')

    def merge3(self, fn, my, other):
        """perform a 3-way merge in the working directory"""

        def temp(prefix, node):
            pre = "%s~%s." % (os.path.basename(fn), prefix)
            (fd, name) = tempfile.mkstemp("", pre)
            f = os.fdopen(fd, "wb")
            f.write(fl.revision(node))
            f.close()
            return name

        fl = self.file(fn)
        base = fl.ancestor(my, other)
        a = self.wjoin(fn)
        b = temp("base", base)
        c = temp("other", other)

        self.ui.note("resolving %s\n" % fn)
        self.ui.debug("file %s: other %s ancestor %s\n" %
                              (fn, short(other), short(base)))

        cmd = (os.environ.get("HGMERGE") or self.ui.config("ui", "merge")
               or "hgmerge")
        r = os.system("%s %s %s %s" % (cmd, a, b, c))
        if r:
            self.ui.warn("merging %s failed!\n" % fn)

        os.unlink(b)
        os.unlink(c)

    def verify(self):
        filelinkrevs = {}
        filenodes = {}
        changesets = revisions = files = 0
        errors = 0

        seen = {}
        self.ui.status("checking changesets\n")
        for i in range(self.changelog.count()):
            changesets += 1
            n = self.changelog.node(i)
            if n in seen:
                self.ui.warn("duplicate changeset at revision %d\n" % i)
                errors += 1
            seen[n] = 1

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

            for f in changes[3]:
                filelinkrevs.setdefault(f, []).append(i)

        seen = {}
        self.ui.status("checking manifests\n")
        for i in range(self.manifest.count()):
            n = self.manifest.node(i)
            if n in seen:
                self.ui.warn("duplicate manifest at revision %d\n" % i)
                errors += 1
            seen[n] = 1

            for p in self.manifest.parents(n):
                if p not in self.manifest.nodemap:
                    self.ui.warn("manifest %s has unknown parent %s\n" %
                            (short(n), short(p)))
                    errors += 1

            try:
                delta = mdiff.patchtext(self.manifest.delta(n))
            except KeyboardInterrupt:
                self.ui.warn("aborted")
                sys.exit(0)
            except Exception, inst:
                self.ui.warn("unpacking manifest %s: %s\n"
                             % (short(n), inst))
                errors += 1

            ff = [ l.split('\0') for l in delta.splitlines() ]
            for f, fn in ff:
                filenodes.setdefault(f, {})[bin(fn[:40])] = 1

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
            seen = {}
            for i in range(fl.count()):
                revisions += 1
                n = fl.node(i)

                if n in seen:
                    self.ui.warn("%s: duplicate revision %d\n" % (f, i))
                    errors += 1

                if n not in filenodes[f]:
                    self.ui.warn("%s: %d:%s not in manifests\n"
                                 % (f, i, short(n)))
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
                             % (hex(node), f))
                errors += 1

        self.ui.status("%d files, %d changesets, %d total revisions\n" %
                       (files, changesets, revisions))

        if errors:
            self.ui.warn("%d integrity errors encountered!\n" % errors)
            return 1

class httprepository:
    def __init__(self, ui, path):
        # fix missing / after hostname
        s = urlparse.urlsplit(path)
        partial = s[2]
        if not partial: partial = "/"
        self.url = urlparse.urlunsplit((s[0], s[1], partial, '', ''))
        self.ui = ui
        no_list = [ "localhost", "127.0.0.1" ]
        host = ui.config("http_proxy", "host")
        if host is None:
            host = os.environ.get("http_proxy")
        if host and host.startswith('http://'):
            host = host[7:]
        user = ui.config("http_proxy", "user")
        passwd = ui.config("http_proxy", "passwd")
        no = ui.config("http_proxy", "no")
        if no is None:
            no = os.environ.get("no_proxy")
        if no:
            no_list = no_list + no.split(",")

        no_proxy = 0
        for h in no_list:
            if (path.startswith("http://" + h + "/") or
                path.startswith("http://" + h + ":") or
                path == "http://" + h):
                no_proxy = 1

        # Note: urllib2 takes proxy values from the environment and those will
        # take precedence
        for env in ["HTTP_PROXY", "http_proxy", "no_proxy"]:
            if os.environ.has_key(env):
                del os.environ[env]

        proxy_handler = urllib2.BaseHandler()
        if host and not no_proxy:
            proxy_handler = urllib2.ProxyHandler({"http" : "http://" + host})

        authinfo = None
        if user and passwd:
            passmgr = urllib2.HTTPPasswordMgrWithDefaultRealm()
            passmgr.add_password(None, host, user, passwd)
            authinfo = urllib2.ProxyBasicAuthHandler(passmgr)

        opener = urllib2.build_opener(proxy_handler, authinfo)
        urllib2.install_opener(opener)

    def dev(self):
        return -1

    def do_cmd(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        q = {"cmd": cmd}
        q.update(args)
        qs = urllib.urlencode(q)
        cu = "%s?%s" % (self.url, qs)
        resp = urllib2.urlopen(cu)
        proto = resp.headers['content-type']

        # accept old "text/plain" and "application/hg-changegroup" for now
        if not proto.startswith('application/mercurial') and \
               not proto.startswith('text/plain') and \
               not proto.startswith('application/hg-changegroup'):
            raise RepoError("'%s' does not appear to be an hg repository"
                            % self.url)

        if proto.startswith('application/mercurial'):
            version = proto[22:]
            if float(version) > 0.1:
                raise RepoError("'%s' uses newer protocol %s" %
                                (self.url, version))

        return resp

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
        f = self.do_cmd("changegroup", roots=n)
        bytes = 0

        class zread:
            def __init__(self, f):
                self.zd = zlib.decompressobj()
                self.f = f
                self.buf = ""
            def read(self, l):
                while l > len(self.buf):
                    r = self.f.read(4096)
                    if r:
                        self.buf += self.zd.decompress(r)
                    else:
                        self.buf += self.zd.flush()
                        break
                d, self.buf = self.buf[:l], self.buf[l:]
                return d

        return zread(f)

class remotelock:
    def __init__(self, repo):
        self.repo = repo
    def release(self):
        self.repo.unlock()
        self.repo = None
    def __del__(self):
        if self.repo:
            self.release()

class sshrepository:
    def __init__(self, ui, path):
        self.url = path
        self.ui = ui

        m = re.match(r'ssh://(([^@]+)@)?([^:/]+)(:(\d+))?(/(.*))', path)
        if not m:
            raise RepoError("couldn't parse destination %s" % path)

        self.user = m.group(2)
        self.host = m.group(3)
        self.port = m.group(5)
        self.path = m.group(7)

        args = self.user and ("%s@%s" % (self.user, self.host)) or self.host
        args = self.port and ("%s -p %s") % (args, self.port) or args
        path = self.path or ""

        if not path:
            raise RepoError("no remote repository path specified")

        cmd = "ssh %s 'hg -R %s serve --stdio'"
        cmd = cmd % (args, path)

        self.pipeo, self.pipei, self.pipee = os.popen3(cmd)

    def readerr(self):
        while 1:
            r,w,x = select.select([self.pipee], [], [], 0)
            if not r: break
            l = self.pipee.readline()
            if not l: break
            self.ui.status("remote: ", l)

    def __del__(self):
        try:
            self.pipeo.close()
            self.pipei.close()
            for l in self.pipee:
                self.ui.status("remote: ", l)
            self.pipee.close()
        except:
            pass

    def dev(self):
        return -1

    def do_cmd(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        self.pipeo.write("%s\n" % cmd)
        for k, v in args.items():
            self.pipeo.write("%s %d\n" % (k, len(v)))
            self.pipeo.write(v)
        self.pipeo.flush()

        return self.pipei

    def call(self, cmd, **args):
        r = self.do_cmd(cmd, **args)
        l = r.readline()
        self.readerr()
        try:
            l = int(l)
        except:
            raise RepoError("unexpected response '%s'" % l)
        return r.read(l)

    def lock(self):
        self.call("lock")
        return remotelock(self)

    def unlock(self):
        self.call("unlock")

    def heads(self):
        d = self.call("heads")
        try:
            return map(bin, d[:-1].split(" "))
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.call("branches", nodes=n)
        try:
            br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
            return br
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.call("between", pairs=n)
        try:
            p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
            return p
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def changegroup(self, nodes):
        n = " ".join(map(hex, nodes))
        f = self.do_cmd("changegroup", roots=n)
        return self.pipei

    def addchangegroup(self, cg):
        d = self.call("addchangegroup")
        if d:
            raise RepoError("push refused: %s", d)

        while 1:
            d = cg.read(4096)
            if not d: break
            self.pipeo.write(d)
            self.readerr()

        self.pipeo.flush()

        self.readerr()
        l = int(self.pipei.readline())
        return self.pipei.read(l) != ""

def repository(ui, path=None, create=0):
    if path:
        if path.startswith("http://"):
            return httprepository(ui, path)
        if path.startswith("hg://"):
            return httprepository(ui, path.replace("hg://", "http://"))
        if path.startswith("old-http://"):
            return localrepository(ui, path.replace("old-http://", "http://"))
        if path.startswith("ssh://"):
            return sshrepository(ui, path)

    return localrepository(ui, path, create)
