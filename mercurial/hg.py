# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import sys, struct, sha, socket, os, time, base64, re, urllib2
import urllib
from mercurial import byterange
from mercurial.transaction import *
from mercurial.revlog import *

class filelog(revlog):
    def __init__(self, opener, path):
        s = self.encodepath(path)
        revlog.__init__(self, opener, os.path.join("data", s + "i"),
                        os.path.join("data", s))

    def encodepath(self, path):
        s = sha.sha(path).digest()
        s = base64.encodestring(s)[:-3]
        s = re.sub("\+", "%", s)
        s = re.sub("/", "_", s)
        return s

    def read(self, node):
        return self.revision(node)
    def add(self, text, transaction, link, p1=None, p2=None):
        return self.addrevision(text, transaction, link, p1, p2)

    def resolvedag(self, old, new, transaction, link):
        """resolve unmerged heads in our DAG"""
        if old == new: return None
        a = self.ancestor(old, new)
        if old == a: return new
        return self.merge3(old, new, a, transaction, link)

    def merge3(self, my, other, base, transaction, link):
        """perform a 3-way merge and append the result"""
        def temp(prefix, node):
            (fd, name) = tempfile.mkstemp(prefix)
            f = os.fdopen(fd, "w")
            f.write(self.revision(node))
            f.close()
            return name

        a = temp("local", my)
        b = temp("remote", other)
        c = temp("parent", base)

        cmd = os.environ["HGMERGE"]
        r = os.system("%s %s %s %s" % (cmd, a, b, c))
        if r:
            raise "Merge failed, implement rollback!"

        t = open(a).read()
        os.unlink(a)
        os.unlink(b)
        os.unlink(c)
        return self.addrevision(t, transaction, link, my, other)

    def merge(self, other, transaction, linkseq, link):
        """perform a merge and resolve resulting heads"""
        (o, n) = self.mergedag(other, transaction, linkseq)
        return self.resolvedag(o, n, transaction, link)

class manifest(revlog):
    def __init__(self, opener):
        self.mapcache = None
        self.listcache = None
        self.addlist = None
        revlog.__init__(self, opener, "00manifest.i", "00manifest.d")

    def read(self, node):
        if self.mapcache and self.mapcache[0] == node:
            return self.mapcache[1]
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
        if self.listcache and len(self.listcache[0]) == len(a):
            return mdiff.diff(self.listcache[1], self.addlist, 1)
        else:
            return mdiff.diff(a, b)

    def add(self, map, transaction, link, p1=None, p2=None):
        files = map.keys()
        files.sort()

        self.addlist = ["%s\000%s\n" % (f, hex(map[f])) for f in files]
        text = "".join(self.addlist)

        n = self.addrevision(text, transaction, link, p1, p2)
        self.mapcache = (n, map)
        self.listcache = (text, self.addlist)

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

    def add(self, manifest, list, desc, transaction, p1=None, p2=None):
        try: user = os.environ["HGUSER"]
        except: user = os.environ["LOGNAME"] + '@' + socket.getfqdn()
        date = "%d %d" % (time.time(), time.timezone)
        list.sort()
        l = [hex(manifest), user, date] + list + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, self.count(), p1, p2)

    def merge3(self, my, other, base):
        pass

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

        if mode != "r" and os.path.isfile(f):
            s = os.stat(f)
            if s.st_nlink > 1:
                file(f + ".tmp", "w").write(file(f).read())
                os.rename(f+".tmp", f)

        return file(f, mode)

    return o

class repository:
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
                l = open(os.path.join(self.root, ".hgignore")).readlines()
                for pat in l:
                    if pat != "\n":
                        self.ignorelist.append(re.compile(pat[:-1]))
            except IOError: pass
        for pat in self.ignorelist:
            if pat.search(f): return True
        return False

    def join(self, f):
        return os.path.join(self.path, f)

    def file(self, f):
        return filelog(self.opener, f)

    def transaction(self):
        return transaction(self.opener, self.join("journal"))

    def merge(self, other):
        tr = self.transaction()
        changed = {}
        new = {}
        seqrev = self.changelog.count()
        # some magic to allow fiddling in nested scope
        nextrev = [seqrev]

        # helpers for back-linking file revisions to local changeset
        # revisions so we can immediately get to changeset from annotate
        def accumulate(text):
            # track which files are added in which changeset and the
            # corresponding _local_ changeset revision
            files = self.changelog.extract(text)[3]
            for f in files:
                changed.setdefault(f, []).append(nextrev[0])
            nextrev[0] += 1

        def seq(start):
            while 1:
                yield start
                start += 1

        def lseq(l):
            for r in l:
                yield r

        # begin the import/merge of changesets
        self.ui.status("merging new changesets\n")
        (co, cn) = self.changelog.mergedag(other.changelog, tr,
                                           seq(seqrev), accumulate)
        resolverev = self.changelog.count()

        # is there anything to do?
        if co == cn:
            tr.close()
            return
        
        # do we need to resolve?
        simple = (co == self.changelog.ancestor(co, cn))

        # merge all files changed by the changesets,
        # keeping track of the new tips
        changelist = changed.keys()
        changelist.sort()
        for f in changelist:
            sys.stdout.write(".")
            sys.stdout.flush()
            r = self.file(f)
            node = r.merge(other.file(f), tr, lseq(changed[f]), resolverev)
            if node:
                new[f] = node
        sys.stdout.write("\n")

        # begin the merge of the manifest
        self.ui.status("merging manifests\n")
        (mm, mo) = self.manifest.mergedag(other.manifest, tr, seq(seqrev))

        # For simple merges, we don't need to resolve manifests or changesets
        if simple:
            tr.close()
            return

        ma = self.manifest.ancestor(mm, mo)

        # resolve the manifest to point to all the merged files
        self.ui.status("resolving manifests\n")
        mmap = self.manifest.read(mm) # mine
        omap = self.manifest.read(mo) # other
        amap = self.manifest.read(ma) # ancestor
        nmap = {}

        for f, mid in mmap.iteritems():
            if f in omap:
                if mid != omap[f]: 
                    nmap[f] = new.get(f, mid) # use merged version
                else:
                    nmap[f] = new.get(f, mid) # they're the same
                del omap[f]
            elif f in amap:
                if mid != amap[f]: 
                    pass # we should prompt here
                else:
                    pass # other deleted it
            else:
                nmap[f] = new.get(f, mid) # we created it
                
        del mmap

        for f, oid in omap.iteritems():
            if f in amap:
                if oid != amap[f]:
                    pass # this is the nasty case, we should prompt
                else:
                    pass # probably safe
            else:
                nmap[f] = new.get(f, oid) # remote created it

        del omap
        del amap

        node = self.manifest.add(nmap, tr, resolverev, mm, mo)

        # Now all files and manifests are merged, we add the changed files
        # and manifest id to the changelog
        self.ui.status("committing merge changeset\n")
        new = new.keys()
        new.sort()
        if co == cn: cn = -1

        edittext = "\n"+"".join(["HG: changed %s\n" % f for f in new])
        edittext = self.ui.edit(edittext)
        n = self.changelog.add(node, new, edittext, tr, co, cn)

        tr.close()

    def commit(self, parent, update = None, text = ""):
        tr = self.transaction()
        
        try:
            remove = [ l[:-1] for l in self.opener("to-remove") ]
            os.unlink(self.join("to-remove"))

        except IOError:
            remove = []

        if update == None:
            update = self.diffdir(self.root, parent)[0]

        # check in files
        new = {}
        linkrev = self.changelog.count()
        for f in update:
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

        edittext = text + "\n"+"".join(["HG: changed %s\n" % f for f in new])
        edittext += "".join(["HG: removed %s\n" % f for f in remove])
        edittext = self.ui.edit(edittext)

        n = self.changelog.add(mnode, new, edittext, tr)
        tr.close()

        self.setcurrent(n)
        self.dircache.update(new)
        self.dircache.remove(remove)

    def checkdir(self, path):
        d = os.path.dirname(path)
        if not d: return
        if not os.path.isdir(d):
            self.checkdir(d)
            os.mkdir(d)

    def checkout(self, node):
        # checkout is really dumb at the moment
        # it ought to basically merge
        change = self.changelog.read(node)
        mmap = self.manifest.read(change[0])

        l = mmap.keys()
        l.sort()
        stats = []
        for f in l:
            r = self.file(f)
            t = r.revision(mmap[f])
            try:
                file(f, "w").write(t)
            except:
                self.checkdir(f)
                file(f, "w").write(t)

        self.setcurrent(node)
        self.dircache.clear()
        self.dircache.update(l)

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
            t1 = file(fn).read()
            t2 = self.file(fn).revision(mf[fn])
            return cmp(t1, t2)

        for dir, subdirs, files in os.walk(self.root):
            d = dir[len(self.root)+1:]
            if ".hg" in subdirs: subdirs.remove(".hg")
            
            for f in files:
                fn = os.path.join(d, f)
                try: s = os.stat(fn)
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

class ui:
    def __init__(self, verbose=False, debug=False):
        self.verbose = verbose
    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))
    def prompt(self, msg, pat):
        while 1:
            sys.stdout.write(msg)
            r = sys.stdin.readline()[:-1]
            if re.match(pat, r):
                return r
    def status(self, *msg):
        self.write(*msg)
    def warn(self, msg):
        self.write(*msg)
    def note(self, msg):
        if self.verbose: self.write(*msg)
    def debug(self, msg):
        if self.debug: self.write(*msg)
    def edit(self, text):
        (fd, name) = tempfile.mkstemp("hg")
        f = os.fdopen(fd, "w")
        f.write(text)
        f.close()

        editor = os.environ.get("EDITOR", "vi")
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
