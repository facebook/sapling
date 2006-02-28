# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import struct, os, util
import filelog, manifest, changelog, dirstate, repo
from node import *
from i18n import gettext as _
from demandload import *
demandload(globals(), "re lock transaction tempfile stat mdiff errno")

class localrepository(object):
    def __del__(self):
        self.transhandle = None
    def __init__(self, ui, path=None, create=0):
        if not path:
            p = os.getcwd()
            while not os.path.isdir(os.path.join(p, ".hg")):
                oldp = p
                p = os.path.dirname(p)
                if p == oldp:
                    raise repo.RepoError(_("no repo found"))
            path = p
        self.path = os.path.join(path, ".hg")

        if not create and not os.path.isdir(self.path):
            raise repo.RepoError(_("repository %s not found") % path)

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
        self.transhandle = None

        if create:
            os.mkdir(self.path)
            os.mkdir(self.join("data"))

        self.dirstate = dirstate.dirstate(self.opener, ui, self.root)
        try:
            self.ui.readconfig(self.join("hgrc"))
        except IOError:
            pass

    def hook(self, name, throw=False, **args):
        def runhook(name, cmd):
            self.ui.note(_("running hook %s: %s\n") % (name, cmd))
            old = {}
            for k, v in args.items():
                k = k.upper()
                old['HG_' + k] = os.environ.get(k, None)
                old[k] = os.environ.get(k, None)
                os.environ['HG_' + k] = str(v)
                os.environ[k] = str(v)

            try:
                # Hooks run in the repository root
                olddir = os.getcwd()
                os.chdir(self.root)
                r = os.system(cmd)
            finally:
                for k, v in old.items():
                    if v is not None:
                        os.environ[k] = v
                    else:
                        del os.environ[k]

                os.chdir(olddir)

            if r:
                desc, r = util.explain_exit(r)
                if throw:
                    raise util.Abort(_('%s hook %s') % (name, desc))
                self.ui.warn(_('error: %s hook %s\n') % (name, desc))
                return False
            return True

        r = True
        for hname, cmd in self.ui.configitems("hooks"):
            s = hname.split(".")
            if s[0] == name and cmd:
                r = runhook(hname, cmd) and r
        return r

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
            l.append((r, t, n))
        l.sort()
        return [(t, n) for r, t, n in l]

    def nodetags(self, node):
        '''return the tags associated with a node'''
        if not self.nodetagscache:
            self.nodetagscache = {}
            for t, n in self.tags().items():
                self.nodetagscache.setdefault(n, []).append(t)
        return self.nodetagscache.get(node, [])

    def lookup(self, key):
        try:
            return self.tags()[key]
        except KeyError:
            try:
                return self.changelog.lookup(key)
            except:
                raise repo.RepoError(_("unknown revision '%s'") % key)

    def dev(self):
        return os.stat(self.path).st_dev

    def local(self):
        return True

    def join(self, f):
        return os.path.join(self.path, f)

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/':
            f = f[1:]
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
                self.ui.debug(_("filtering %s through %s\n") % (filename, cmd))
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
                self.ui.debug(_("filtering %s through %s\n") % (filename, cmd))
                data = util.filter(data, cmd)
                break

        if fd:
            return fd.write(data)
        return self.wopener(filename, 'w').write(data)

    def transaction(self):
        tr = self.transhandle
        if tr != None and tr.running():
            return tr.nest()

        # save dirstate for undo
        try:
            ds = self.opener("dirstate").read()
        except IOError:
            ds = ""
        self.opener("journal.dirstate", "w").write(ds)

        tr = transaction.transaction(self.ui.warn, self.opener,
                                       self.join("journal"), 
                                       aftertrans(self.path))
        self.transhandle = tr
        return tr

    def recover(self):
        l = self.lock()
        if os.path.exists(self.join("journal")):
            self.ui.status(_("rolling back interrupted transaction\n"))
            transaction.rollback(self.opener, self.join("journal"))
            self.reload()
            return True
        else:
            self.ui.warn(_("no interrupted transaction available\n"))
            return False

    def undo(self, wlock=None):
        if not wlock:
            wlock = self.wlock()
        l = self.lock()
        if os.path.exists(self.join("undo")):
            self.ui.status(_("rolling back last transaction\n"))
            transaction.rollback(self.opener, self.join("undo"))
            util.rename(self.join("undo.dirstate"), self.join("dirstate"))
            self.reload()
            self.wreload()
        else:
            self.ui.warn(_("no undo information available\n"))

    def wreload(self):
        self.dirstate.read()

    def reload(self):
        self.changelog.load()
        self.manifest.load()
        self.tagscache = None
        self.nodetagscache = None

    def do_lock(self, lockname, wait, releasefn=None, acquirefn=None):
        try:
            l = lock.lock(self.join(lockname), 0, releasefn)
        except lock.LockHeld, inst:
            if not wait:
                raise inst
            self.ui.warn(_("waiting for lock held by %s\n") % inst.args[0])
            try:
                # default to 600 seconds timeout
                l = lock.lock(self.join(lockname),
                              int(self.ui.config("ui", "timeout") or 600),
                              releasefn)
            except lock.LockHeld, inst:
                raise util.Abort(_("timeout while waiting for "
                                   "lock held by %s") % inst.args[0])
        if acquirefn:
            acquirefn()
        return l

    def lock(self, wait=1):
        return self.do_lock("lock", wait, acquirefn=self.reload)

    def wlock(self, wait=1):
        return self.do_lock("wlock", wait,
                            self.dirstate.write,
                            self.wreload)

    def checkfilemerge(self, filename, text, filelog, manifest1, manifest2):
        "determine whether a new filenode is needed"
        fp1 = manifest1.get(filename, nullid)
        fp2 = manifest2.get(filename, nullid)

        if fp2 != nullid:
            # is one parent an ancestor of the other?
            fpa = filelog.ancestor(fp1, fp2)
            if fpa == fp1:
                fp1, fp2 = fp2, nullid
            elif fpa == fp2:
                fp2 = nullid

            # is the file unmodified from the parent? report existing entry
            if fp2 == nullid and text == filelog.read(fp1):
                return (fp1, None, None)

        return (None, fp1, fp2)

    def rawcommit(self, files, text, user, date, p1=None, p2=None, wlock=None):
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

        if not wlock:
            wlock = self.wlock()
        l = self.lock()
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

                (entry, fp1, fp2) = self.checkfilemerge(f, t, r, m1, m2)
                if entry:
                    mm[f] = entry
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

    def commit(self, files=None, text="", user=None, date=None,
               match=util.always, force=False, wlock=None):
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
                    self.ui.warn(_("%s not tracked!\n") % f)
        else:
            modified, added, removed, deleted, unknown = self.changes(match=match)
            commit = modified + added
            remove = removed

        p1, p2 = self.dirstate.parents()
        c1 = self.changelog.read(p1)
        c2 = self.changelog.read(p2)
        m1 = self.manifest.read(c1[0])
        mf1 = self.manifest.readflags(c1[0])
        m2 = self.manifest.read(c2[0])

        if not commit and not remove and not force and p2 == nullid:
            self.ui.status(_("nothing changed\n"))
            return None

        xp1 = hex(p1)
        if p2 == nullid: xp2 = ''
        else: xp2 = hex(p2)

        self.hook("precommit", throw=True, parent1=xp1, parent2=xp2)

        if not wlock:
            wlock = self.wlock()
        l = self.lock()
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
                self.ui.warn(_("trouble committing %s!\n") % f)
                raise

            r = self.file(f)

            meta = {}
            cp = self.dirstate.copied(f)
            if cp:
                meta["copy"] = cp
                meta["copyrev"] = hex(m1.get(cp, m2.get(cp, nullid)))
                self.ui.debug(_(" %s: copy %s:%s\n") % (f, cp, meta["copyrev"]))
                fp1, fp2 = nullid, nullid
            else:
                entry, fp1, fp2 = self.checkfilemerge(f, t, r, m1, m2)
                if entry:
                    new[f] = entry
                    continue

            new[f] = r.add(t, meta, tr, linkrev, fp1, fp2)
            # remember what we've added so that we can later calculate
            # the files to pull from a set of changesets
            changed.append(f)

        # update manifest
        m1 = m1.copy()
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
            edittext = [""]
            if p2 != nullid:
                edittext.append("HG: branch merge")
            edittext.extend(["HG: changed %s" % f for f in changed])
            edittext.extend(["HG: removed %s" % f for f in remove])
            if not changed and not remove:
                edittext.append("HG: no files changed")
            edittext.append("")
            # run editor in the repository root
            olddir = os.getcwd()
            os.chdir(self.root)
            edittext = self.ui.edit("\n".join(edittext))
            os.chdir(olddir)
            if not edittext.rstrip():
                return None
            text = edittext

        user = user or self.ui.username()
        n = self.changelog.add(mn, changed + remove, text, tr, p1, p2, user, date)
        self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                  parent2=xp2)
        tr.close()

        self.dirstate.setparents(n)
        self.dirstate.update(new, "n")
        self.dirstate.forget(remove)

        self.hook("commit", node=hex(n), parent1=xp1, parent2=xp2)
        return n

    def walk(self, node=None, files=[], match=util.always):
        if node:
            fdict = dict.fromkeys(files)
            for fn in self.manifest.read(self.changelog.read(node)[0]):
                fdict.pop(fn, None)
                if match(fn):
                    yield 'm', fn
            for fn in fdict:
                self.ui.warn(_('%s: No such file in rev %s\n') % (
                    util.pathto(self.getcwd(), fn), short(node)))
        else:
            for src, fn in self.dirstate.walk(files, match):
                yield src, fn

    def changes(self, node1=None, node2=None, files=[], match=util.always,
                wlock=None):
        """return changes between two nodes or node and working directory

        If node1 is None, use the first dirstate parent instead.
        If node2 is None, compare node1 with working directory.
        """

        def fcmp(fn, mf):
            t1 = self.wread(fn)
            t2 = self.file(fn).read(mf.get(fn, nullid))
            return cmp(t1, t2)

        def mfmatches(node):
            change = self.changelog.read(node)
            mf = dict(self.manifest.read(change[0]))
            for fn in mf.keys():
                if not match(fn):
                    del mf[fn]
            return mf

        if node1:
            # read the manifest from node1 before the manifest from node2,
            # so that we'll hit the manifest cache if we're going through
            # all the revisions in parent->child order.
            mf1 = mfmatches(node1)

        # are we comparing the working directory?
        if not node2:
            if not wlock:
                try:
                    wlock = self.wlock(wait=0)
                except lock.LockException:
                    wlock = None
            lookup, modified, added, removed, deleted, unknown = (
                self.dirstate.changes(files, match))

            # are we comparing working dir against its parent?
            if not node1:
                if lookup:
                    # do a full compare of any files that might have changed
                    mf2 = mfmatches(self.dirstate.parents()[0])
                    for f in lookup:
                        if fcmp(f, mf2):
                            modified.append(f)
                        elif wlock is not None:
                            self.dirstate.update([f], "n")
            else:
                # we are comparing working dir against non-parent
                # generate a pseudo-manifest for the working dir
                mf2 = mfmatches(self.dirstate.parents()[0])
                for f in lookup + modified + added:
                    mf2[f] = ""
                for f in removed:
                    if f in mf2:
                        del mf2[f]
        else:
            # we are comparing two revisions
            deleted, unknown = [], []
            mf2 = mfmatches(node2)

        if node1:
            # flush lists from dirstate before comparing manifests
            modified, added = [], []

            for fn in mf2:
                if mf1.has_key(fn):
                    if mf1[fn] != mf2[fn] and (mf2[fn] != "" or fcmp(fn, mf1)):
                        modified.append(fn)
                    del mf1[fn]
                else:
                    added.append(fn)

            removed = mf1.keys()

        # sort and return results:
        for l in modified, added, removed, deleted, unknown:
            l.sort()
        return (modified, added, removed, deleted, unknown)

    def add(self, list, wlock=None):
        if not wlock:
            wlock = self.wlock()
        for f in list:
            p = self.wjoin(f)
            if not os.path.exists(p):
                self.ui.warn(_("%s does not exist!\n") % f)
            elif not os.path.isfile(p):
                self.ui.warn(_("%s not added: only files supported currently\n")
                             % f)
            elif self.dirstate.state(f) in 'an':
                self.ui.warn(_("%s already tracked!\n") % f)
            else:
                self.dirstate.update([f], "a")

    def forget(self, list, wlock=None):
        if not wlock:
            wlock = self.wlock()
        for f in list:
            if self.dirstate.state(f) not in 'ai':
                self.ui.warn(_("%s not added!\n") % f)
            else:
                self.dirstate.forget([f])

    def remove(self, list, unlink=False, wlock=None):
        if unlink:
            for f in list:
                try:
                    util.unlink(self.wjoin(f))
                except OSError, inst:
                    if inst.errno != errno.ENOENT:
                        raise
        if not wlock:
            wlock = self.wlock()
        for f in list:
            p = self.wjoin(f)
            if os.path.exists(p):
                self.ui.warn(_("%s still exists!\n") % f)
            elif self.dirstate.state(f) == 'a':
                self.dirstate.forget([f])
            elif f not in self.dirstate:
                self.ui.warn(_("%s not tracked!\n") % f)
            else:
                self.dirstate.update([f], "r")

    def undelete(self, list, wlock=None):
        p = self.dirstate.parents()[0]
        mn = self.changelog.read(p)[0]
        mf = self.manifest.readflags(mn)
        m = self.manifest.read(mn)
        if not wlock:
            wlock = self.wlock()
        for f in list:
            if self.dirstate.state(f) not in  "r":
                self.ui.warn("%s not removed!\n" % f)
            else:
                t = self.file(f).read(m[f])
                self.wwrite(f, t)
                util.set_exec(self.wjoin(f), mf[f])
                self.dirstate.update([f], "n")

    def copy(self, source, dest, wlock=None):
        p = self.wjoin(dest)
        if not os.path.exists(p):
            self.ui.warn(_("%s does not exist!\n") % dest)
        elif not os.path.isfile(p):
            self.ui.warn(_("copy failed: %s is not a file\n") % dest)
        else:
            if not wlock:
                wlock = self.wlock()
            if self.dirstate.state(dest) == '?':
                self.dirstate.update([dest], "a")
            self.dirstate.copy(source, dest)

    def heads(self, start=None):
        heads = self.changelog.heads(start)
        # sort the output in rev descending order
        heads = [(-self.changelog.rev(h), h) for h in heads]
        heads.sort()
        return [n for (r, n) in heads]

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
        if not nodes:
            nodes = [self.changelog.tip()]
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
        self.ui.status(_("searching for changes\n"))

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

                self.ui.debug(_("examining %s:%s\n")
                              % (short(n[0]), short(n[1])))
                if n[0] == nullid:
                    break
                if n in seenbranch:
                    self.ui.debug(_("branch already found\n"))
                    continue
                if n[1] and n[1] in m: # do we know the base?
                    self.ui.debug(_("found incomplete branch %s:%s\n")
                                  % (short(n[0]), short(n[1])))
                    search.append(n) # schedule branch range for scanning
                    seenbranch[n] = 1
                else:
                    if n[1] not in seen and n[1] not in fetch:
                        if n[2] in m and n[3] in m:
                            self.ui.debug(_("found new changeset %s\n") %
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
                self.ui.debug(_("request %d: %s\n") %
                            (reqcnt, " ".join(map(short, r))))
                for p in range(0, len(r), 10):
                    for b in remote.branches(r[p:p+10]):
                        self.ui.debug(_("received %s:%s\n") %
                                      (short(b[0]), short(b[1])))
                        if b[0] in m:
                            self.ui.debug(_("found base node %s\n")
                                          % short(b[0]))
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
                self.ui.debug(_("narrowing %d:%d %s\n") % (f, len(l), short(i)))
                if i in m:
                    if f <= 2:
                        self.ui.debug(_("found new branch changeset %s\n") %
                                          short(p))
                        fetch[p] = 1
                        base[i] = 1
                    else:
                        self.ui.debug(_("narrowed branch search to %s:%s\n")
                                      % (short(p), short(i)))
                        search.append((p, i))
                    break
                p, f = i, f * 2

        # sanity check our fetch list
        for f in fetch.keys():
            if f in m:
                raise repo.RepoError(_("already have changeset ") + short(f[:4]))

        if base.keys() == [nullid]:
            self.ui.warn(_("warning: pulling from an unrelated repository!\n"))

        self.ui.note(_("found new changesets starting at ") +
                     " ".join([short(f) for f in fetch]) + "\n")

        self.ui.debug(_("%d total queries\n") % reqcnt)

        return fetch.keys()

    def findoutgoing(self, remote, base=None, heads=None):
        if base == None:
            base = {}
            self.findincoming(remote, base, heads)

        self.ui.debug(_("common changesets up to ")
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

    def pull(self, remote, heads=None):
        l = self.lock()

        # if we have an empty repo, fetch everything
        if self.changelog.tip() == nullid:
            self.ui.status(_("requesting all changes\n"))
            fetch = [nullid]
        else:
            fetch = self.findincoming(remote)

        if not fetch:
            self.ui.status(_("no changes found\n"))
            return 1

        if heads is None:
            cg = remote.changegroup(fetch, 'pull')
        else:
            cg = remote.changegroupsubset(fetch, heads, 'pull')
        return self.addchangegroup(cg)

    def push(self, remote, force=False, revs=None):
        lock = remote.lock()

        base = {}
        heads = remote.heads()
        inc = self.findincoming(remote, base, heads)
        if not force and inc:
            self.ui.warn(_("abort: unsynced remote changes!\n"))
            self.ui.status(_("(did you forget to sync? use push -f to force)\n"))
            return 1

        update = self.findoutgoing(remote, base)
        if revs is not None:
            msng_cl, bases, heads = self.changelog.nodesbetween(update, revs)
        else:
            bases, heads = update, self.changelog.heads()

        if not bases:
            self.ui.status(_("no changes found\n"))
            return 1
        elif not force:
            if len(bases) < len(heads):
                self.ui.warn(_("abort: push creates new remote branches!\n"))
                self.ui.status(_("(did you forget to merge?"
                                 " use push -f to force)\n"))
                return 1

        if revs is None:
            cg = self.changegroup(update, 'push')
        else:
            cg = self.changegroupsubset(update, revs, 'push')
        return remote.addchangegroup(cg)

    def changegroupsubset(self, bases, heads, source):
        """This function generates a changegroup consisting of all the nodes
        that are descendents of any of the bases, and ancestors of any of
        the heads.

        It is fairly complex as determining which filenodes and which
        manifest nodes need to be included for the changeset to be complete
        is non-trivial.

        Another wrinkle is doing the reverse, figuring out which changeset in
        the changegroup a particular filenode or manifestnode belongs to."""

        self.hook('preoutgoing', throw=True, source=source)

        # Set up some initial variables
        # Make it easy to refer to self.changelog
        cl = self.changelog
        # msng is short for missing - compute the list of changesets in this
        # changegroup.
        msng_cl_lst, bases, heads = cl.nodesbetween(bases, heads)
        # Some bases may turn out to be superfluous, and some heads may be
        # too.  nodesbetween will return the minimal set of bases and heads
        # necessary to re-create the changegroup.

        # Known heads are the list of heads that it is assumed the recipient
        # of this changegroup will know about.
        knownheads = {}
        # We assume that all parents of bases are known heads.
        for n in bases:
            for p in cl.parents(n):
                if p != nullid:
                    knownheads[p] = 1
        knownheads = knownheads.keys()
        if knownheads:
            # Now that we know what heads are known, we can compute which
            # changesets are known.  The recipient must know about all
            # changesets required to reach the known heads from the null
            # changeset.
            has_cl_set, junk, junk = cl.nodesbetween(None, knownheads)
            junk = None
            # Transform the list into an ersatz set.
            has_cl_set = dict.fromkeys(has_cl_set)
        else:
            # If there were no known heads, the recipient cannot be assumed to
            # know about any changesets.
            has_cl_set = {}

        # Make it easy to refer to self.manifest
        mnfst = self.manifest
        # We don't know which manifests are missing yet
        msng_mnfst_set = {}
        # Nor do we know which filenodes are missing.
        msng_filenode_set = {}

        junk = mnfst.index[mnfst.count() - 1] # Get around a bug in lazyindex
        junk = None

        # A changeset always belongs to itself, so the changenode lookup
        # function for a changenode is identity.
        def identity(x):
            return x

        # A function generating function.  Sets up an environment for the
        # inner function.
        def cmp_by_rev_func(revlog):
            # Compare two nodes by their revision number in the environment's
            # revision history.  Since the revision number both represents the
            # most efficient order to read the nodes in, and represents a
            # topological sorting of the nodes, this function is often useful.
            def cmp_by_rev(a, b):
                return cmp(revlog.rev(a), revlog.rev(b))
            return cmp_by_rev

        # If we determine that a particular file or manifest node must be a
        # node that the recipient of the changegroup will already have, we can
        # also assume the recipient will have all the parents.  This function
        # prunes them from the set of missing nodes.
        def prune_parents(revlog, hasset, msngset):
            haslst = hasset.keys()
            haslst.sort(cmp_by_rev_func(revlog))
            for node in haslst:
                parentlst = [p for p in revlog.parents(node) if p != nullid]
                while parentlst:
                    n = parentlst.pop()
                    if n not in hasset:
                        hasset[n] = 1
                        p = [p for p in revlog.parents(n) if p != nullid]
                        parentlst.extend(p)
            for n in hasset:
                msngset.pop(n, None)

        # This is a function generating function used to set up an environment
        # for the inner function to execute in.
        def manifest_and_file_collector(changedfileset):
            # This is an information gathering function that gathers
            # information from each changeset node that goes out as part of
            # the changegroup.  The information gathered is a list of which
            # manifest nodes are potentially required (the recipient may
            # already have them) and total list of all files which were
            # changed in any changeset in the changegroup.
            #
            # We also remember the first changenode we saw any manifest
            # referenced by so we can later determine which changenode 'owns'
            # the manifest.
            def collect_manifests_and_files(clnode):
                c = cl.read(clnode)
                for f in c[3]:
                    # This is to make sure we only have one instance of each
                    # filename string for each filename.
                    changedfileset.setdefault(f, f)
                msng_mnfst_set.setdefault(c[0], clnode)
            return collect_manifests_and_files

        # Figure out which manifest nodes (of the ones we think might be part
        # of the changegroup) the recipient must know about and remove them
        # from the changegroup.
        def prune_manifests():
            has_mnfst_set = {}
            for n in msng_mnfst_set:
                # If a 'missing' manifest thinks it belongs to a changenode
                # the recipient is assumed to have, obviously the recipient
                # must have that manifest.
                linknode = cl.node(mnfst.linkrev(n))
                if linknode in has_cl_set:
                    has_mnfst_set[n] = 1
            prune_parents(mnfst, has_mnfst_set, msng_mnfst_set)

        # Use the information collected in collect_manifests_and_files to say
        # which changenode any manifestnode belongs to.
        def lookup_manifest_link(mnfstnode):
            return msng_mnfst_set[mnfstnode]

        # A function generating function that sets up the initial environment
        # the inner function.
        def filenode_collector(changedfiles):
            next_rev = [0]
            # This gathers information from each manifestnode included in the
            # changegroup about which filenodes the manifest node references
            # so we can include those in the changegroup too.
            #
            # It also remembers which changenode each filenode belongs to.  It
            # does this by assuming the a filenode belongs to the changenode
            # the first manifest that references it belongs to.
            def collect_msng_filenodes(mnfstnode):
                r = mnfst.rev(mnfstnode)
                if r == next_rev[0]:
                    # If the last rev we looked at was the one just previous,
                    # we only need to see a diff.
                    delta = mdiff.patchtext(mnfst.delta(mnfstnode))
                    # For each line in the delta
                    for dline in delta.splitlines():
                        # get the filename and filenode for that line
                        f, fnode = dline.split('\0')
                        fnode = bin(fnode[:40])
                        f = changedfiles.get(f, None)
                        # And if the file is in the list of files we care
                        # about.
                        if f is not None:
                            # Get the changenode this manifest belongs to
                            clnode = msng_mnfst_set[mnfstnode]
                            # Create the set of filenodes for the file if
                            # there isn't one already.
                            ndset = msng_filenode_set.setdefault(f, {})
                            # And set the filenode's changelog node to the
                            # manifest's if it hasn't been set already.
                            ndset.setdefault(fnode, clnode)
                else:
                    # Otherwise we need a full manifest.
                    m = mnfst.read(mnfstnode)
                    # For every file in we care about.
                    for f in changedfiles:
                        fnode = m.get(f, None)
                        # If it's in the manifest
                        if fnode is not None:
                            # See comments above.
                            clnode = msng_mnfst_set[mnfstnode]
                            ndset = msng_filenode_set.setdefault(f, {})
                            ndset.setdefault(fnode, clnode)
                # Remember the revision we hope to see next.
                next_rev[0] = r + 1
            return collect_msng_filenodes

        # We have a list of filenodes we think we need for a file, lets remove
        # all those we now the recipient must have.
        def prune_filenodes(f, filerevlog):
            msngset = msng_filenode_set[f]
            hasset = {}
            # If a 'missing' filenode thinks it belongs to a changenode we
            # assume the recipient must have, then the recipient must have
            # that filenode.
            for n in msngset:
                clnode = cl.node(filerevlog.linkrev(n))
                if clnode in has_cl_set:
                    hasset[n] = 1
            prune_parents(filerevlog, hasset, msngset)

        # A function generator function that sets up the a context for the
        # inner function.
        def lookup_filenode_link_func(fname):
            msngset = msng_filenode_set[fname]
            # Lookup the changenode the filenode belongs to.
            def lookup_filenode_link(fnode):
                return msngset[fnode]
            return lookup_filenode_link

        # Now that we have all theses utility functions to help out and
        # logically divide up the task, generate the group.
        def gengroup():
            # The set of changed files starts empty.
            changedfiles = {}
            # Create a changenode group generator that will call our functions
            # back to lookup the owning changenode and collect information.
            group = cl.group(msng_cl_lst, identity,
                             manifest_and_file_collector(changedfiles))
            for chnk in group:
                yield chnk

            # The list of manifests has been collected by the generator
            # calling our functions back.
            prune_manifests()
            msng_mnfst_lst = msng_mnfst_set.keys()
            # Sort the manifestnodes by revision number.
            msng_mnfst_lst.sort(cmp_by_rev_func(mnfst))
            # Create a generator for the manifestnodes that calls our lookup
            # and data collection functions back.
            group = mnfst.group(msng_mnfst_lst, lookup_manifest_link,
                                filenode_collector(changedfiles))
            for chnk in group:
                yield chnk

            # These are no longer needed, dereference and toss the memory for
            # them.
            msng_mnfst_lst = None
            msng_mnfst_set.clear()

            changedfiles = changedfiles.keys()
            changedfiles.sort()
            # Go through all our files in order sorted by name.
            for fname in changedfiles:
                filerevlog = self.file(fname)
                # Toss out the filenodes that the recipient isn't really
                # missing.
                if msng_filenode_set.has_key(fname):
                    prune_filenodes(fname, filerevlog)
                    msng_filenode_lst = msng_filenode_set[fname].keys()
                else:
                    msng_filenode_lst = []
                # If any filenodes are left, generate the group for them,
                # otherwise don't bother.
                if len(msng_filenode_lst) > 0:
                    yield struct.pack(">l", len(fname) + 4) + fname
                    # Sort the filenodes by their revision #
                    msng_filenode_lst.sort(cmp_by_rev_func(filerevlog))
                    # Create a group generator and only pass in a changenode
                    # lookup function as we need to collect no information
                    # from filenodes.
                    group = filerevlog.group(msng_filenode_lst,
                                             lookup_filenode_link_func(fname))
                    for chnk in group:
                        yield chnk
                if msng_filenode_set.has_key(fname):
                    # Don't need this anymore, toss it to free memory.
                    del msng_filenode_set[fname]
            # Signal that no more groups are left.
            yield struct.pack(">l", 0)

            self.hook('outgoing', node=hex(msng_cl_lst[0]), source=source)

        return util.chunkbuffer(gengroup())

    def changegroup(self, basenodes, source):
        """Generate a changegroup of all nodes that we have that a recipient
        doesn't.

        This is much easier than the previous function as we can assume that
        the recipient has any changenode we aren't sending them."""

        self.hook('preoutgoing', throw=True, source=source)

        cl = self.changelog
        nodes = cl.nodesbetween(basenodes, None)[0]
        revset = dict.fromkeys([cl.rev(n) for n in nodes])

        def identity(x):
            return x

        def gennodelst(revlog):
            for r in xrange(0, revlog.count()):
                n = revlog.node(r)
                if revlog.linkrev(n) in revset:
                    yield n

        def changed_file_collector(changedfileset):
            def collect_changed_files(clnode):
                c = cl.read(clnode)
                for fname in c[3]:
                    changedfileset[fname] = 1
            return collect_changed_files

        def lookuprevlink_func(revlog):
            def lookuprevlink(n):
                return cl.node(revlog.linkrev(n))
            return lookuprevlink

        def gengroup():
            # construct a list of all changed files
            changedfiles = {}

            for chnk in cl.group(nodes, identity,
                                 changed_file_collector(changedfiles)):
                yield chnk
            changedfiles = changedfiles.keys()
            changedfiles.sort()

            mnfst = self.manifest
            nodeiter = gennodelst(mnfst)
            for chnk in mnfst.group(nodeiter, lookuprevlink_func(mnfst)):
                yield chnk

            for fname in changedfiles:
                filerevlog = self.file(fname)
                nodeiter = gennodelst(filerevlog)
                nodeiter = list(nodeiter)
                if nodeiter:
                    yield struct.pack(">l", len(fname) + 4) + fname
                    lookup = lookuprevlink_func(filerevlog)
                    for chnk in filerevlog.group(nodeiter, lookup):
                        yield chnk

            yield struct.pack(">l", 0)
            self.hook('outgoing', node=hex(nodes[0]), source=source)

        return util.chunkbuffer(gengroup())

    def addchangegroup(self, source):

        def getchunk():
            d = source.read(4)
            if not d:
                return ""
            l = struct.unpack(">l", d)[0]
            if l <= 4:
                return ""
            d = source.read(l - 4)
            if len(d) < l - 4:
                raise repo.RepoError(_("premature EOF reading chunk"
                                       " (got %d bytes, expected %d)")
                                     % (len(d), l - 4))
            return d

        def getgroup():
            while 1:
                c = getchunk()
                if not c:
                    break
                yield c

        def csmap(x):
            self.ui.debug(_("add changeset %s\n") % short(x))
            return self.changelog.count()

        def revmap(x):
            return self.changelog.rev(x)

        if not source:
            return

        self.hook('prechangegroup', throw=True)

        changesets = files = revisions = 0

        tr = self.transaction()

        oldheads = len(self.changelog.heads())

        # pull off the changeset group
        self.ui.status(_("adding changesets\n"))
        co = self.changelog.tip()
        cn = self.changelog.addgroup(getgroup(), csmap, tr, 1) # unique
        cnr, cor = map(self.changelog.rev, (cn, co))
        if cn == nullid:
            cnr = cor
        changesets = cnr - cor

        # pull off the manifest group
        self.ui.status(_("adding manifests\n"))
        mm = self.manifest.tip()
        mo = self.manifest.addgroup(getgroup(), revmap, tr)

        # process the files
        self.ui.status(_("adding file changes\n"))
        while 1:
            f = getchunk()
            if not f:
                break
            self.ui.debug(_("adding %s revisions\n") % f)
            fl = self.file(f)
            o = fl.count()
            n = fl.addgroup(getgroup(), revmap, tr)
            revisions += fl.count() - o
            files += 1

        newheads = len(self.changelog.heads())
        heads = ""
        if oldheads and newheads > oldheads:
            heads = _(" (+%d heads)") % (newheads - oldheads)

        self.ui.status(_("added %d changesets"
                         " with %d changes to %d files%s\n")
                         % (changesets, revisions, files, heads))

        self.hook('pretxnchangegroup', throw=True,
                  node=hex(self.changelog.node(cor+1)))

        tr.close()

        if changesets > 0:
            self.hook("changegroup", node=hex(self.changelog.node(cor+1)))

            for i in range(cor + 1, cnr + 1):
                self.hook("incoming", node=hex(self.changelog.node(i)))

    def update(self, node, allow=False, force=False, choose=None,
               moddirstate=True, forcemerge=False, wlock=None):
        pl = self.dirstate.parents()
        if not force and pl[1] != nullid:
            self.ui.warn(_("aborting: outstanding uncommitted merges\n"))
            return 1

        err = False

        p1, p2 = pl[0], node
        pa = self.changelog.ancestor(p1, p2)
        m1n = self.changelog.read(p1)[0]
        m2n = self.changelog.read(p2)[0]
        man = self.manifest.ancestor(m1n, m2n)
        m1 = self.manifest.read(m1n)
        mf1 = self.manifest.readflags(m1n)
        m2 = self.manifest.read(m2n).copy()
        mf2 = self.manifest.readflags(m2n)
        ma = self.manifest.read(man)
        mfa = self.manifest.readflags(man)

        modified, added, removed, deleted, unknown = self.changes()

        # is this a jump, or a merge?  i.e. is there a linear path
        # from p1 to p2?
        linear_path = (pa == p1 or pa == p2)

        if allow and linear_path:
            raise util.Abort(_("there is nothing to merge, "
                               "just use 'hg update'"))
        if allow and not forcemerge:
            if modified or added or removed:
                raise util.Abort(_("outstanding uncommited changes"))
        if not forcemerge and not force:
            for f in unknown:
                if f in m2:
                    t1 = self.wread(f)
                    t2 = self.file(f).read(m2[f])
                    if cmp(t1, t2) != 0:
                        raise util.Abort(_("'%s' already exists in the working"
                                           " dir and differs from remote") % f)

        # resolve the manifest to determine which files
        # we care about merging
        self.ui.note(_("resolving manifests\n"))
        self.ui.debug(_(" force %s allow %s moddirstate %s linear %s\n") %
                      (force, allow, moddirstate, linear_path))
        self.ui.debug(_(" ancestor %s local %s remote %s\n") %
                      (short(man), short(m1n), short(m2n)))

        merge = {}
        get = {}
        remove = []

        # construct a working dir manifest
        mw = m1.copy()
        mfw = mf1.copy()
        umap = dict.fromkeys(unknown)

        for f in added + modified + unknown:
            mw[f] = ""
            mfw[f] = util.is_exec(self.wjoin(f), mfw.get(f, False))

        if moddirstate and not wlock:
            wlock = self.wlock()

        for f in deleted + removed:
            if f in mw:
                del mw[f]

            # If we're jumping between revisions (as opposed to merging),
            # and if neither the working directory nor the target rev has
            # the file, then we need to remove it from the dirstate, to
            # prevent the dirstate from listing the file when it is no
            # longer in the manifest.
            if moddirstate and linear_path and f not in m2:
                self.dirstate.forget((f,))

        # Compare manifests
        for f, n in mw.iteritems():
            if choose and not choose(f):
                continue
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
                        self.ui.debug(_(" %s versions differ, resolve\n") % f)
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
                        self.ui.debug(_(" remote %s is newer, get\n") % f)
                        get[f] = m2[f]
                        s = 1
                elif f in umap:
                    # this unknown file is the same as the checkout
                    get[f] = m2[f]

                if not s and mfw[f] != mf2[f]:
                    if force:
                        self.ui.debug(_(" updating permissions for %s\n") % f)
                        util.set_exec(self.wjoin(f), mf2[f])
                    else:
                        a, b, c = mfa.get(f, 0), mfw[f], mf2[f]
                        mode = ((a^b) | (a^c)) ^ a
                        if mode != b:
                            self.ui.debug(_(" updating permissions for %s\n")
                                          % f)
                            util.set_exec(self.wjoin(f), mode)
                del m2[f]
            elif f in ma:
                if n != ma[f]:
                    r = _("d")
                    if not force and (linear_path or allow):
                        r = self.ui.prompt(
                            (_(" local changed %s which remote deleted\n") % f) +
                             _("(k)eep or (d)elete?"), _("[kd]"), _("k"))
                    if r == _("d"):
                        remove.append(f)
                else:
                    self.ui.debug(_("other deleted %s\n") % f)
                    remove.append(f) # other deleted it
            else:
                # file is created on branch or in working directory
                if force and f not in umap:
                    self.ui.debug(_("remote deleted %s, clobbering\n") % f)
                    remove.append(f)
                elif n == m1.get(f, nullid): # same as parent
                    if p2 == pa: # going backwards?
                        self.ui.debug(_("remote deleted %s\n") % f)
                        remove.append(f)
                    else:
                        self.ui.debug(_("local modified %s, keeping\n") % f)
                else:
                    self.ui.debug(_("working dir created %s, keeping\n") % f)

        for f, n in m2.iteritems():
            if choose and not choose(f):
                continue
            if f[0] == "/":
                continue
            if f in ma and n != ma[f]:
                r = _("k")
                if not force and (linear_path or allow):
                    r = self.ui.prompt(
                        (_("remote changed %s which local deleted\n") % f) +
                         _("(k)eep or (d)elete?"), _("[kd]"), _("k"))
                if r == _("k"):
                    get[f] = n
            elif f not in ma:
                self.ui.debug(_("remote created %s\n") % f)
                get[f] = n
            else:
                if force or p2 == pa: # going backwards?
                    self.ui.debug(_("local deleted %s, recreating\n") % f)
                    get[f] = n
                else:
                    self.ui.debug(_("local deleted %s\n") % f)

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
                self.ui.status(_("this update spans a branch"
                                 " affecting the following files:\n"))
                fl = merge.keys() + get.keys()
                fl.sort()
                for f in fl:
                    cf = ""
                    if f in merge:
                        cf = _(" (resolve)")
                    self.ui.status(" %s%s\n" % (f, cf))
                self.ui.warn(_("aborting update spanning branches!\n"))
                self.ui.status(_("(use update -m to merge across branches"
                                 " or -C to lose changes)\n"))
                return 1
            branch_merge = True

        # get the files we don't need to change
        files = get.keys()
        files.sort()
        for f in files:
            if f[0] == "/":
                continue
            self.ui.note(_("getting %s\n") % f)
            t = self.file(f).read(get[f])
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
            self.ui.status(_("merging %s\n") % f)
            my, other, flag = merge[f]
            ret = self.merge3(f, my, other)
            if ret:
                err = True
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
            self.ui.note(_("removing %s\n") % f)
            try:
                util.unlink(self.wjoin(f))
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    self.ui.warn(_("update failed to remove %s: %s!\n") %
                                 (f, inst.strerror))
        if moddirstate:
            if branch_merge:
                self.dirstate.update(remove, 'r')
            else:
                self.dirstate.forget(remove)

        if moddirstate:
            self.dirstate.setparents(p1, p2)
        return err

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

        self.ui.note(_("resolving %s\n") % fn)
        self.ui.debug(_("file %s: my %s other %s ancestor %s\n") %
                              (fn, short(my), short(other), short(base)))

        cmd = (os.environ.get("HGMERGE") or self.ui.config("ui", "merge")
               or "hgmerge")
        r = os.system('%s "%s" "%s" "%s"' % (cmd, a, b, c))
        if r:
            self.ui.warn(_("merging %s failed!\n") % fn)

        os.unlink(b)
        os.unlink(c)
        return r

    def verify(self):
        filelinkrevs = {}
        filenodes = {}
        changesets = revisions = files = 0
        errors = [0]
        neededmanifests = {}

        def err(msg):
            self.ui.warn(msg + "\n")
            errors[0] += 1

        def checksize(obj, name):
            d = obj.checksize()
            if d[0]:
                err(_("%s data length off by %d bytes") % (name, d[0]))
            if d[1]:
                err(_("%s index contains %d extra bytes") % (name, d[1]))

        seen = {}
        self.ui.status(_("checking changesets\n"))
        checksize(self.changelog, "changelog")

        for i in range(self.changelog.count()):
            changesets += 1
            n = self.changelog.node(i)
            l = self.changelog.linkrev(n)
            if l != i:
                err(_("incorrect link (%d) for changeset revision %d") %(l, i))
            if n in seen:
                err(_("duplicate changeset at revision %d") % i)
            seen[n] = 1

            for p in self.changelog.parents(n):
                if p not in self.changelog.nodemap:
                    err(_("changeset %s has unknown parent %s") %
                                 (short(n), short(p)))
            try:
                changes = self.changelog.read(n)
            except KeyboardInterrupt:
                self.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(_("unpacking changeset %s: %s") % (short(n), inst))

            neededmanifests[changes[0]] = n

            for f in changes[3]:
                filelinkrevs.setdefault(f, []).append(i)

        seen = {}
        self.ui.status(_("checking manifests\n"))
        checksize(self.manifest, "manifest")

        for i in range(self.manifest.count()):
            n = self.manifest.node(i)
            l = self.manifest.linkrev(n)

            if l < 0 or l >= self.changelog.count():
                err(_("bad manifest link (%d) at revision %d") % (l, i))

            if n in neededmanifests:
                del neededmanifests[n]

            if n in seen:
                err(_("duplicate manifest at revision %d") % i)

            seen[n] = 1

            for p in self.manifest.parents(n):
                if p not in self.manifest.nodemap:
                    err(_("manifest %s has unknown parent %s") %
                        (short(n), short(p)))

            try:
                delta = mdiff.patchtext(self.manifest.delta(n))
            except KeyboardInterrupt:
                self.ui.warn(_("interrupted"))
                raise
            except Exception, inst:
                err(_("unpacking manifest %s: %s") % (short(n), inst))

            ff = [ l.split('\0') for l in delta.splitlines() ]
            for f, fn in ff:
                filenodes.setdefault(f, {})[bin(fn[:40])] = 1

        self.ui.status(_("crosschecking files in changesets and manifests\n"))

        for m, c in neededmanifests.items():
            err(_("Changeset %s refers to unknown manifest %s") %
                (short(m), short(c)))
        del neededmanifests

        for f in filenodes:
            if f not in filelinkrevs:
                err(_("file %s in manifest but not in changesets") % f)

        for f in filelinkrevs:
            if f not in filenodes:
                err(_("file %s in changeset but not in manifest") % f)

        self.ui.status(_("checking files\n"))
        ff = filenodes.keys()
        ff.sort()
        for f in ff:
            if f == "/dev/null":
                continue
            files += 1
            fl = self.file(f)
            checksize(fl, f)

            nodes = {nullid: 1}
            seen = {}
            for i in range(fl.count()):
                revisions += 1
                n = fl.node(i)

                if n in seen:
                    err(_("%s: duplicate revision %d") % (f, i))
                if n not in filenodes[f]:
                    err(_("%s: %d:%s not in manifests") % (f, i, short(n)))
                else:
                    del filenodes[f][n]

                flr = fl.linkrev(n)
                if flr not in filelinkrevs[f]:
                    err(_("%s:%s points to unexpected changeset %d")
                            % (f, short(n), flr))
                else:
                    filelinkrevs[f].remove(flr)

                # verify contents
                try:
                    t = fl.read(n)
                except KeyboardInterrupt:
                    self.ui.warn(_("interrupted"))
                    raise
                except Exception, inst:
                    err(_("unpacking file %s %s: %s") % (f, short(n), inst))

                # verify parents
                (p1, p2) = fl.parents(n)
                if p1 not in nodes:
                    err(_("file %s:%s unknown parent 1 %s") %
                        (f, short(n), short(p1)))
                if p2 not in nodes:
                    err(_("file %s:%s unknown parent 2 %s") %
                            (f, short(n), short(p1)))
                nodes[n] = 1

            # cross-check
            for node in filenodes[f]:
                err(_("node %s in manifests not in %s") % (hex(node), f))

        self.ui.status(_("%d files, %d changesets, %d total revisions\n") %
                       (files, changesets, revisions))

        if errors[0]:
            self.ui.warn(_("%d integrity errors encountered!\n") % errors[0])
            return 1

# used to avoid circular references so destructors work
def aftertrans(base):
    p = base
    def a():
        util.rename(os.path.join(p, "journal"), os.path.join(p, "undo"))
        util.rename(os.path.join(p, "journal.dirstate"),
                    os.path.join(p, "undo.dirstate"))
    return a

