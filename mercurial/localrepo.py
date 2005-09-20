# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import struct, os, util
import filelog, manifest, changelog, dirstate, repo
from node import *
from demandload import *
demandload(globals(), "re lock transaction tempfile stat mdiff")

class localrepository:
    def __init__(self, ui, path=None, create=0):
        if not path:
            p = os.getcwd()
            while not os.path.isdir(os.path.join(p, ".hg")):
                oldp = p
                p = os.path.dirname(p)
                if p == oldp: raise repo.RepoError("no repo found")
            path = p
        self.path = os.path.join(path, ".hg")

        if not create and not os.path.isdir(self.path):
            raise repo.RepoError("repository %s not found" % self.path)

        self.root = os.path.abspath(path)
        self.ui = ui
        self.opener = util.opener(self.path)
        self.wopener = util.opener(self.root)
        self.manifest = manifest.manifest(self.opener)
        self.changelog = changelog.changelog(self.opener)
        self.tagscache = None
        self.nodetagscache = None
        self.encodepats = None
        self.decodepats = None

        if create:
            os.mkdir(self.path)
            os.mkdir(self.join("data"))

        self.dirstate = dirstate.dirstate(self.opener, ui, self.root)
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
                    for l in fl.read(r).splitlines():
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
                raise repo.RepoError("unknown revision '%s'" % key)

    def dev(self):
        return os.stat(self.path).st_dev

    def local(self):
        return True

    def join(self, f):
        return os.path.join(self.path, f)

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/': f = f[1:]
        return filelog.filelog(self.opener, f)

    def getcwd(self):
        return self.dirstate.getcwd()

    def wfile(self, f, mode='r'):
        return self.wopener(f, mode)

    def wread(self, filename):
        if self.encodepats == None:
            l = []
            for pat, cmd in self.ui.configitems("encode"):
                mf = util.matcher("", "/", [pat], [], [])[1]
                l.append((mf, cmd))
            self.encodepats = l

        data = self.wopener(filename, 'r').read()

        for mf, cmd in self.encodepats:
            if mf(filename):
                self.ui.debug("filtering %s through %s\n" % (filename, cmd))
                data = util.filter(data, cmd)
                break

        return data

    def wwrite(self, filename, data, fd=None):
        if self.decodepats == None:
            l = []
            for pat, cmd in self.ui.configitems("decode"):
                mf = util.matcher("", "/", [pat], [], [])[1]
                l.append((mf, cmd))
            self.decodepats = l

        for mf, cmd in self.decodepats:
            if mf(filename):
                self.ui.debug("filtering %s through %s\n" % (filename, cmd))
                data = util.filter(data, cmd)
                break

        if fd:
            return fd.write(data)
        return self.wopener(filename, 'w').write(data)

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
            self.dirstate = dirstate.dirstate(self.opener, self.ui, self.root)
        else:
            self.ui.warn("no undo information available\n")

    def lock(self, wait=1):
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
        changed = []

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
                t = self.wread(f)
                tm = util.is_exec(self.wjoin(f), mfm.get(f, False))
                r = self.file(f)
                mfm[f] = tm

                fp1 = m1.get(f, nullid)
                fp2 = m2.get(f, nullid)

                # is the same revision on two branches of a merge?
                if fp2 == fp1:
                    fp2 = nullid

                if fp2 != nullid:
                    # is one parent an ancestor of the other?
                    fpa = r.ancestor(fp1, fp2)
                    if fpa == fp1:
                        fp1, fp2 = fp2, nullid
                    elif fpa == fp2:
                        fp2 = nullid

                    # is the file unmodified from the parent?
                    if t == r.read(fp1):
                        # record the proper existing parent in manifest
                        # no need to add a revision
                        mm[f] = fp1
                        continue

                mm[f] = r.add(t, {}, tr, linkrev, fp1, fp2)
                changed.append(f)
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
        n = self.changelog.add(mnode, changed, text, tr, p1, p2, user, date)
        tr.close()
        if update_dirstate:
            self.dirstate.setparents(n, nullid)

    def commit(self, files = None, text = "", user = None, date = None,
               match = util.always, force=False):
        commit = []
        remove = []
        changed = []

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
            (c, a, d, u) = self.changes(match=match)
            commit = c + a
            remove = d

        p1, p2 = self.dirstate.parents()
        c1 = self.changelog.read(p1)
        c2 = self.changelog.read(p2)
        m1 = self.manifest.read(c1[0])
        mf1 = self.manifest.readflags(c1[0])
        m2 = self.manifest.read(c2[0])

        if not commit and not remove and not force and p2 == nullid:
            self.ui.status("nothing changed\n")
            return None

        if not self.hook("precommit"):
            return None

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
                t = self.wread(f)
            except IOError:
                self.ui.warn("trouble committing %s!\n" % f)
                raise

            r = self.file(f)

            meta = {}
            cp = self.dirstate.copied(f)
            if cp:
                meta["copy"] = cp
                meta["copyrev"] = hex(m1.get(cp, m2.get(cp, nullid)))
                self.ui.debug(" %s: copy %s:%s\n" % (f, cp, meta["copyrev"]))
                fp1, fp2 = nullid, nullid
            else:
                fp1 = m1.get(f, nullid)
                fp2 = m2.get(f, nullid)

            # is the same revision on two branches of a merge?
            if fp2 == fp1:
                fp2 = nullid

            if fp2 != nullid:
                # is one parent an ancestor of the other?
                fpa = r.ancestor(fp1, fp2)
                if fpa == fp1:
                    fp1, fp2 = fp2, nullid
                elif fpa == fp2:
                    fp2 = nullid

                # is the file unmodified from the parent?
                if not meta and t == r.read(fp1):
                    # record the proper existing parent in manifest
                    # no need to add a revision
                    new[f] = fp1
                    continue

            new[f] = r.add(t, meta, tr, linkrev, fp1, fp2)
            # remember what we've added so that we can later calculate
            # the files to pull from a set of changesets
            changed.append(f)

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
            edittext = ""
            if p2 != nullid:
                edittext += "HG: branch merge\n"
            edittext += "\n" + "HG: manifest hash %s\n" % hex(mn)
            edittext += "".join(["HG: changed %s\n" % f for f in changed])
            edittext += "".join(["HG: removed %s\n" % f for f in remove])
            if not changed and not remove:
                edittext += "HG: no files changed\n"
            edittext = self.ui.edit(edittext)
            if not edittext.rstrip():
                return None
            text = edittext

        user = user or self.ui.username()
        n = self.changelog.add(mn, changed, text, tr, p1, p2, user, date)
        tr.close()

        self.dirstate.setparents(n)
        self.dirstate.update(new, "n")
        self.dirstate.forget(remove)

        if not self.hook("commit", node=hex(n)):
            return None
        return n

    def walk(self, node=None, files=[], match=util.always):
        if node:
            for fn in self.manifest.read(self.changelog.read(node)[0]):
                if match(fn): yield 'm', fn
        else:
            for src, fn in self.dirstate.walk(files, match):
                yield src, fn

    def changes(self, node1 = None, node2 = None, files = [],
                match = util.always):
        mf2, u = None, []

        def fcmp(fn, mf):
            t1 = self.wread(fn)
            t2 = self.file(fn).read(mf.get(fn, nullid))
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

    # branchlookup returns a dict giving a list of branches for
    # each head.  A branch is defined as the tag of a node or
    # the branch of the node's parents.  If a node has multiple
    # branch tags, tags are eliminated if they are visible from other
    # branch tags.
    #
    # So, for this graph:  a->b->c->d->e
    #                       \         /
    #                        aa -----/
    # a has tag 2.6.12
    # d has tag 2.6.13
    # e would have branch tags for 2.6.12 and 2.6.13.  Because the node
    # for 2.6.12 can be reached from the node 2.6.13, that is eliminated
    # from the list.
    #
    # It is possible that more than one head will have the same branch tag.
    # callers need to check the result for multiple heads under the same
    # branch tag if that is a problem for them (ie checkout of a specific
    # branch).
    #
    # passing in a specific branch will limit the depth of the search
    # through the parents.  It won't limit the branches returned in the
    # result though.
    def branchlookup(self, heads=None, branch=None):
        if not heads:
            heads = self.heads()
        headt = [ h for h in heads ]
        chlog = self.changelog
        branches = {}
        merges = []
        seenmerge = {}

        # traverse the tree once for each head, recording in the branches
        # dict which tags are visible from this head.  The branches
        # dict also records which tags are visible from each tag
        # while we traverse.
        while headt or merges:
            if merges:
                n, found = merges.pop()
                visit = [n]
            else:
                h = headt.pop()
                visit = [h]
                found = [h]
                seen = {}
            while visit:
                n = visit.pop()
                if n in seen:
                    continue
                pp = chlog.parents(n)
                tags = self.nodetags(n)
                if tags:
                    for x in tags:
                        if x == 'tip':
                            continue
                        for f in found:
                            branches.setdefault(f, {})[n] = 1
                        branches.setdefault(n, {})[n] = 1
                        break
                    if n not in found:
                        found.append(n)
                    if branch in tags:
                        continue
                seen[n] = 1
                if pp[1] != nullid and n not in seenmerge:
                    merges.append((pp[1], [x for x in found]))
                    seenmerge[n] = 1
                if pp[0] != nullid:
                    visit.append(pp[0])
        # traverse the branches dict, eliminating branch tags from each
        # head that are visible from another branch tag for that head.
        out = {}
        viscache = {}
        for h in heads:
            def visible(node):
                if node in viscache:
                    return viscache[node]
                ret = {}
                visit = [node]
                while visit:
                    x = visit.pop()
                    if x in viscache:
                        ret.update(viscache[x])
                    elif x not in ret:
                        ret[x] = 1
                        if x in branches:
                            visit[len(visit):] = branches[x].keys()
                viscache[node] = ret
                return ret
            if h not in branches:
                continue
            # O(n^2), but somewhat limited.  This only searches the
            # tags visible from a specific head, not all the tags in the
            # whole repo.
            for b in branches[h]:
                vis = False
                for bb in branches[h].keys():
                    if b != bb:
                        if b in visible(bb):
                            vis = True
                            break
                if not vis:
                    l = out.setdefault(h, [])
                    l[len(l):] = self.nodetags(b)
        return out

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
        fetch = {}
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
                            fetch[n[1]] = 1 # earliest unknown
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
                        if b[0] in m:
                            self.ui.debug("found base node %s\n" % short(b[0]))
                            base[b[0]] = 1
                        elif b[0] not in seen:
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
                        fetch[p] = 1
                        base[i] = 1
                    else:
                        self.ui.debug("narrowed branch search to %s:%s\n"
                                      % (short(p), short(i)))
                        search.append((p, i))
                    break
                p, f = i, f * 2

        # sanity check our fetch list
        for f in fetch.keys():
            if f in m:
                raise repo.RepoError("already have changeset " + short(f[:4]))

        if base.keys() == [nullid]:
            self.ui.warn("warning: pulling from an unrelated repository!\n")

        self.ui.note("found new changesets starting at " +
                     " ".join([short(f) for f in fetch]) + "\n")

        self.ui.debug("%d total queries\n" % reqcnt)

        return fetch.keys()

    def findoutgoing(self, remote, base=None, heads=None):
        if base == None:
            base = {}
            self.findincoming(remote, base, heads)

        self.ui.debug("common changesets up to "
                      + " ".join(map(short, base.keys())) + "\n")

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
        genread = util.chunkbuffer

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
            d = source.read(l - 4)
            if len(d) < l - 4:
                raise repo.RepoError("premature EOF reading chunk" +
                                     " (got %d bytes, expected %d)"
                                     % (len(d), l - 4))
            return d

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

        oldheads = len(self.changelog.heads())

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

        newheads = len(self.changelog.heads())
        heads = ""
        if oldheads and newheads > oldheads:
            heads = " (+%d heads)" % (newheads - oldheads)

        self.ui.status(("added %d changesets" +
                        " with %d changes to %d files%s\n")
                       % (changesets, revisions, files, heads))

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
                    t1 = self.wread(f)
                    t2 = self.file(f).read(m2[f])
                    if cmp(t1, t2) == 0:
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
                # file is created on branch or in working directory
                if force and f not in umap:
                    self.ui.debug("remote deleted %s, clobbering\n" % f)
                    remove.append(f)
                elif n == m1.get(f, nullid): # same as parent
                    if p2 == pa: # going backwards?
                        self.ui.debug("remote deleted %s\n" % f)
                        remove.append(f)
                    else:
                        self.ui.debug("local modified %s, keeping\n" % f)
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
            branch_merge = False
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
            branch_merge = True

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
                self.wwrite(f, t)
            except IOError:
                os.makedirs(os.path.dirname(self.wjoin(f)))
                self.wwrite(f, t)
            util.set_exec(self.wjoin(f), mf2[f])
            if moddirstate:
                if branch_merge:
                    self.dirstate.update([f], 'n', st_mtime=-1)
                else:
                    self.dirstate.update([f], 'n')

        # merge the tricky bits
        files = merge.keys()
        files.sort()
        for f in files:
            self.ui.status("merging %s\n" % f)
            my, other, flag = merge[f]
            self.merge3(f, my, other)
            util.set_exec(self.wjoin(f), flag)
            if moddirstate:
                if branch_merge:
                    # We've done a branch merge, mark this file as merged
                    # so that we properly record the merger later
                    self.dirstate.update([f], 'm')
                else:
                    # We've update-merged a locally modified file, so
                    # we set the dirstate to emulate a normal checkout
                    # of that file some time in the past. Thus our
                    # merge will appear as a normal local file
                    # modification.
                    f_len = len(self.file(f).read(other))
                    self.dirstate.update([f], 'n', st_size=f_len, st_mtime=-1)

        remove.sort()
        for f in remove:
            self.ui.note("removing %s\n" % f)
            try:
                os.unlink(self.wjoin(f))
            except OSError, inst:
                self.ui.warn("update failed to remove %s: %s!\n" % (f, inst))
            # try removing directories that might now be empty
            try: os.removedirs(os.path.dirname(self.wjoin(f)))
            except: pass
        if moddirstate:
            if branch_merge:
                self.dirstate.update(remove, 'r')
            else:
                self.dirstate.forget(remove)

    def merge3(self, fn, my, other):
        """perform a 3-way merge in the working directory"""

        def temp(prefix, node):
            pre = "%s~%s." % (os.path.basename(fn), prefix)
            (fd, name) = tempfile.mkstemp("", pre)
            f = os.fdopen(fd, "wb")
            self.wwrite(fn, fl.read(node), f)
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
                self.ui.warn("interrupted")
                raise
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
