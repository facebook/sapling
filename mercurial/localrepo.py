# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import bin, hex, nullid, nullrev, short
from i18n import _
import repo, changegroup
import changelog, dirstate, filelog, manifest, context, weakref
import lock, transaction, stat, errno, ui
import os, revlog, time, util, extensions, hook, inspect

class localrepository(repo.repository):
    capabilities = util.set(('lookup', 'changegroupsubset'))
    supported = ('revlogv1', 'store')

    def __init__(self, parentui, path=None, create=0):
        repo.repository.__init__(self)
        self.root = os.path.realpath(path)
        self.path = os.path.join(self.root, ".hg")
        self.origroot = path
        self.opener = util.opener(self.path)
        self.wopener = util.opener(self.root)

        if not os.path.isdir(self.path):
            if create:
                if not os.path.exists(path):
                    os.mkdir(path)
                os.mkdir(self.path)
                requirements = ["revlogv1"]
                if parentui.configbool('format', 'usestore', True):
                    os.mkdir(os.path.join(self.path, "store"))
                    requirements.append("store")
                    # create an invalid changelog
                    self.opener("00changelog.i", "a").write(
                        '\0\0\0\2' # represents revlogv2
                        ' dummy changelog to prevent using the old repo layout'
                    )
                reqfile = self.opener("requires", "w")
                for r in requirements:
                    reqfile.write("%s\n" % r)
                reqfile.close()
            else:
                raise repo.RepoError(_("repository %s not found") % path)
        elif create:
            raise repo.RepoError(_("repository %s already exists") % path)
        else:
            # find requirements
            try:
                requirements = self.opener("requires").read().splitlines()
            except IOError, inst:
                if inst.errno != errno.ENOENT:
                    raise
                requirements = []
        # check them
        for r in requirements:
            if r not in self.supported:
                raise repo.RepoError(_("requirement '%s' not supported") % r)

        # setup store
        if "store" in requirements:
            self.encodefn = util.encodefilename
            self.decodefn = util.decodefilename
            self.spath = os.path.join(self.path, "store")
        else:
            self.encodefn = lambda x: x
            self.decodefn = lambda x: x
            self.spath = self.path

        try:
            # files in .hg/ will be created using this mode
            mode = os.stat(self.spath).st_mode
            # avoid some useless chmods
            if (0777 & ~util._umask) == (0777 & mode):
                mode = None
        except OSError:
            mode = None

        self._createmode = mode
        self.opener.createmode = mode
        sopener = util.opener(self.spath)
        sopener.createmode = mode
        self.sopener = util.encodedopener(sopener, self.encodefn)

        self.ui = ui.ui(parentui=parentui)
        try:
            self.ui.readconfig(self.join("hgrc"), self.root)
            extensions.loadall(self.ui)
        except IOError:
            pass

        self.tagscache = None
        self._tagstypecache = None
        self.branchcache = None
        self._ubranchcache = None  # UTF-8 version of branchcache
        self._branchcachetip = None
        self.nodetagscache = None
        self.filterpats = {}
        self._datafilters = {}
        self._transref = self._lockref = self._wlockref = None

    def __getattr__(self, name):
        if name == 'changelog':
            self.changelog = changelog.changelog(self.sopener)
            self.sopener.defversion = self.changelog.version
            return self.changelog
        if name == 'manifest':
            self.changelog
            self.manifest = manifest.manifest(self.sopener)
            return self.manifest
        if name == 'dirstate':
            self.dirstate = dirstate.dirstate(self.opener, self.ui, self.root)
            return self.dirstate
        else:
            raise AttributeError, name

    def url(self):
        return 'file:' + self.root

    def hook(self, name, throw=False, **args):
        return hook.hook(self.ui, self, name, throw, **args)

    tag_disallowed = ':\r\n'

    def _tag(self, names, node, message, local, user, date, parent=None,
             extra={}):
        use_dirstate = parent is None

        if isinstance(names, str):
            allchars = names
            names = (names,)
        else:
            allchars = ''.join(names)
        for c in self.tag_disallowed:
            if c in allchars:
                raise util.Abort(_('%r cannot be used in a tag name') % c)

        for name in names:
            self.hook('pretag', throw=True, node=hex(node), tag=name,
                      local=local)

        def writetags(fp, names, munge, prevtags):
            fp.seek(0, 2)
            if prevtags and prevtags[-1] != '\n':
                fp.write('\n')
            for name in names:
                fp.write('%s %s\n' % (hex(node), munge and munge(name) or name))
            fp.close()

        prevtags = ''
        if local:
            try:
                fp = self.opener('localtags', 'r+')
            except IOError, err:
                fp = self.opener('localtags', 'a')
            else:
                prevtags = fp.read()

            # local tags are stored in the current charset
            writetags(fp, names, None, prevtags)
            for name in names:
                self.hook('tag', node=hex(node), tag=name, local=local)
            return

        if use_dirstate:
            try:
                fp = self.wfile('.hgtags', 'rb+')
            except IOError, err:
                fp = self.wfile('.hgtags', 'ab')
            else:
                prevtags = fp.read()
        else:
            try:
                prevtags = self.filectx('.hgtags', parent).data()
            except revlog.LookupError:
                pass
            fp = self.wfile('.hgtags', 'wb')
            if prevtags:
                fp.write(prevtags)

        # committed tags are stored in UTF-8
        writetags(fp, names, util.fromlocal, prevtags)

        if use_dirstate and '.hgtags' not in self.dirstate:
            self.add(['.hgtags'])

        tagnode = self.commit(['.hgtags'], message, user, date, p1=parent,
                              extra=extra)

        for name in names:
            self.hook('tag', node=hex(node), tag=name, local=local)

        return tagnode

    def tag(self, names, node, message, local, user, date):
        '''tag a revision with one or more symbolic names.

        names is a list of strings or, when adding a single tag, names may be a
        string.

        if local is True, the tags are stored in a per-repository file.
        otherwise, they are stored in the .hgtags file, and a new
        changeset is committed with the change.

        keyword arguments:

        local: whether to store tags in non-version-controlled file
        (default False)

        message: commit message to use if committing

        user: name of user to use if committing

        date: date tuple to use if committing'''

        for x in self.status()[:5]:
            if '.hgtags' in x:
                raise util.Abort(_('working copy of .hgtags is changed '
                                   '(please commit .hgtags manually)'))

        self._tag(names, node, message, local, user, date)

    def tags(self):
        '''return a mapping of tag to node'''
        if self.tagscache:
            return self.tagscache

        globaltags = {}
        tagtypes = {}

        def readtags(lines, fn, tagtype):
            filetags = {}
            count = 0

            def warn(msg):
                self.ui.warn(_("%s, line %s: %s\n") % (fn, count, msg))

            for l in lines:
                count += 1
                if not l:
                    continue
                s = l.split(" ", 1)
                if len(s) != 2:
                    warn(_("cannot parse entry"))
                    continue
                node, key = s
                key = util.tolocal(key.strip()) # stored in UTF-8
                try:
                    bin_n = bin(node)
                except TypeError:
                    warn(_("node '%s' is not well formed") % node)
                    continue
                if bin_n not in self.changelog.nodemap:
                    warn(_("tag '%s' refers to unknown node") % key)
                    continue

                h = []
                if key in filetags:
                    n, h = filetags[key]
                    h.append(n)
                filetags[key] = (bin_n, h)

            for k, nh in filetags.items():
                if k not in globaltags:
                    globaltags[k] = nh
                    tagtypes[k] = tagtype
                    continue

                # we prefer the global tag if:
                #  it supercedes us OR
                #  mutual supercedes and it has a higher rank
                # otherwise we win because we're tip-most
                an, ah = nh
                bn, bh = globaltags[k]
                if (bn != an and an in bh and
                    (bn not in ah or len(bh) > len(ah))):
                    an = bn
                ah.extend([n for n in bh if n not in ah])
                globaltags[k] = an, ah
                tagtypes[k] = tagtype

        # read the tags file from each head, ending with the tip
        f = None
        for rev, node, fnode in self._hgtagsnodes():
            f = (f and f.filectx(fnode) or
                 self.filectx('.hgtags', fileid=fnode))
            readtags(f.data().splitlines(), f, "global")

        try:
            data = util.fromlocal(self.opener("localtags").read())
            # localtags are stored in the local character set
            # while the internal tag table is stored in UTF-8
            readtags(data.splitlines(), "localtags", "local")
        except IOError:
            pass

        self.tagscache = {}
        self._tagstypecache = {}
        for k,nh in globaltags.items():
            n = nh[0]
            if n != nullid:
                self.tagscache[k] = n
                self._tagstypecache[k] = tagtypes[k]
        self.tagscache['tip'] = self.changelog.tip()

        return self.tagscache

    def tagtype(self, tagname):
        '''
        return the type of the given tag. result can be:

        'local'  : a local tag
        'global' : a global tag
        None     : tag does not exist
        '''

        self.tags()

        return self._tagstypecache.get(tagname)

    def _hgtagsnodes(self):
        heads = self.heads()
        heads.reverse()
        last = {}
        ret = []
        for node in heads:
            c = self.changectx(node)
            rev = c.rev()
            try:
                fnode = c.filenode('.hgtags')
            except revlog.LookupError:
                continue
            ret.append((rev, node, fnode))
            if fnode in last:
                ret[last[fnode]] = None
            last[fnode] = len(ret) - 1
        return [item for item in ret if item]

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

    def _branchtags(self, partial, lrev):
        tiprev = self.changelog.count() - 1
        if lrev != tiprev:
            self._updatebranchcache(partial, lrev+1, tiprev+1)
            self._writebranchcache(partial, self.changelog.tip(), tiprev)

        return partial

    def branchtags(self):
        tip = self.changelog.tip()
        if self.branchcache is not None and self._branchcachetip == tip:
            return self.branchcache

        oldtip = self._branchcachetip
        self._branchcachetip = tip
        if self.branchcache is None:
            self.branchcache = {} # avoid recursion in changectx
        else:
            self.branchcache.clear() # keep using the same dict
        if oldtip is None or oldtip not in self.changelog.nodemap:
            partial, last, lrev = self._readbranchcache()
        else:
            lrev = self.changelog.rev(oldtip)
            partial = self._ubranchcache

        self._branchtags(partial, lrev)

        # the branch cache is stored on disk as UTF-8, but in the local
        # charset internally
        for k, v in partial.items():
            self.branchcache[util.tolocal(k)] = v
        self._ubranchcache = partial
        return self.branchcache

    def _readbranchcache(self):
        partial = {}
        try:
            f = self.opener("branch.cache")
            lines = f.read().split('\n')
            f.close()
        except (IOError, OSError):
            return {}, nullid, nullrev

        try:
            last, lrev = lines.pop(0).split(" ", 1)
            last, lrev = bin(last), int(lrev)
            if not (lrev < self.changelog.count() and
                    self.changelog.node(lrev) == last): # sanity check
                # invalidate the cache
                raise ValueError('invalidating branch cache (tip differs)')
            for l in lines:
                if not l: continue
                node, label = l.split(" ", 1)
                partial[label.strip()] = bin(node)
        except (KeyboardInterrupt, util.SignalInterrupt):
            raise
        except Exception, inst:
            if self.ui.debugflag:
                self.ui.warn(str(inst), '\n')
            partial, last, lrev = {}, nullid, nullrev
        return partial, last, lrev

    def _writebranchcache(self, branches, tip, tiprev):
        try:
            f = self.opener("branch.cache", "w", atomictemp=True)
            f.write("%s %s\n" % (hex(tip), tiprev))
            for label, node in branches.iteritems():
                f.write("%s %s\n" % (hex(node), label))
            f.rename()
        except (IOError, OSError):
            pass

    def _updatebranchcache(self, partial, start, end):
        for r in xrange(start, end):
            c = self.changectx(r)
            b = c.branch()
            partial[b] = c.node()

    def lookup(self, key):
        if key == '.':
            key, second = self.dirstate.parents()
            if key == nullid:
                raise repo.RepoError(_("no revision checked out"))
            if second != nullid:
                self.ui.warn(_("warning: working directory has two parents, "
                               "tag '.' uses the first\n"))
        elif key == 'null':
            return nullid
        n = self.changelog._match(key)
        if n:
            return n
        if key in self.tags():
            return self.tags()[key]
        if key in self.branchtags():
            return self.branchtags()[key]
        n = self.changelog._partialmatch(key)
        if n:
            return n
        try:
            if len(key) == 20:
                key = hex(key)
        except:
            pass
        raise repo.RepoError(_("unknown revision '%s'") % key)

    def local(self):
        return True

    def join(self, f):
        return os.path.join(self.path, f)

    def sjoin(self, f):
        f = self.encodefn(f)
        return os.path.join(self.spath, f)

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/':
            f = f[1:]
        return filelog.filelog(self.sopener, f)

    def changectx(self, changeid=None):
        return context.changectx(self, changeid)

    def workingctx(self):
        return context.workingctx(self)

    def parents(self, changeid=None):
        '''
        get list of changectxs for parents of changeid or working directory
        '''
        if changeid is None:
            pl = self.dirstate.parents()
        else:
            n = self.changelog.lookup(changeid)
            pl = self.changelog.parents(n)
        if pl[1] == nullid:
            return [self.changectx(pl[0])]
        return [self.changectx(pl[0]), self.changectx(pl[1])]

    def filectx(self, path, changeid=None, fileid=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        return context.filectx(self, path, changeid, fileid)

    def getcwd(self):
        return self.dirstate.getcwd()

    def pathto(self, f, cwd=None):
        return self.dirstate.pathto(f, cwd)

    def wfile(self, f, mode='r'):
        return self.wopener(f, mode)

    def _link(self, f):
        return os.path.islink(self.wjoin(f))

    def _filter(self, filter, filename, data):
        if filter not in self.filterpats:
            l = []
            for pat, cmd in self.ui.configitems(filter):
                mf = util.matcher(self.root, "", [pat], [], [])[1]
                fn = None
                params = cmd
                for name, filterfn in self._datafilters.iteritems():
                    if cmd.startswith(name):
                        fn = filterfn
                        params = cmd[len(name):].lstrip()
                        break
                if not fn:
                    fn = lambda s, c, **kwargs: util.filter(s, c)
                # Wrap old filters not supporting keyword arguments
                if not inspect.getargspec(fn)[2]:
                    oldfn = fn
                    fn = lambda s, c, **kwargs: oldfn(s, c)
                l.append((mf, fn, params))
            self.filterpats[filter] = l

        for mf, fn, cmd in self.filterpats[filter]:
            if mf(filename):
                self.ui.debug(_("filtering %s through %s\n") % (filename, cmd))
                data = fn(data, cmd, ui=self.ui, repo=self, filename=filename)
                break

        return data

    def adddatafilter(self, name, filter):
        self._datafilters[name] = filter

    def wread(self, filename):
        if self._link(filename):
            data = os.readlink(self.wjoin(filename))
        else:
            data = self.wopener(filename, 'r').read()
        return self._filter("encode", filename, data)

    def wwrite(self, filename, data, flags):
        data = self._filter("decode", filename, data)
        try:
            os.unlink(self.wjoin(filename))
        except OSError:
            pass
        self.wopener(filename, 'w').write(data)
        util.set_flags(self.wjoin(filename), flags)

    def wwritedata(self, filename, data):
        return self._filter("decode", filename, data)

    def transaction(self):
        if self._transref and self._transref():
            return self._transref().nest()

        # abort here if the journal already exists
        if os.path.exists(self.sjoin("journal")):
            raise repo.RepoError(_("journal already exists - run hg recover"))

        # save dirstate for rollback
        try:
            ds = self.opener("dirstate").read()
        except IOError:
            ds = ""
        self.opener("journal.dirstate", "w").write(ds)
        self.opener("journal.branch", "w").write(self.dirstate.branch())

        renames = [(self.sjoin("journal"), self.sjoin("undo")),
                   (self.join("journal.dirstate"), self.join("undo.dirstate")),
                   (self.join("journal.branch"), self.join("undo.branch"))]
        tr = transaction.transaction(self.ui.warn, self.sopener,
                                     self.sjoin("journal"),
                                     aftertrans(renames),
                                     self._createmode)
        self._transref = weakref.ref(tr)
        return tr

    def recover(self):
        l = self.lock()
        try:
            if os.path.exists(self.sjoin("journal")):
                self.ui.status(_("rolling back interrupted transaction\n"))
                transaction.rollback(self.sopener, self.sjoin("journal"))
                self.invalidate()
                return True
            else:
                self.ui.warn(_("no interrupted transaction available\n"))
                return False
        finally:
            del l

    def rollback(self):
        wlock = lock = None
        try:
            wlock = self.wlock()
            lock = self.lock()
            if os.path.exists(self.sjoin("undo")):
                self.ui.status(_("rolling back last transaction\n"))
                transaction.rollback(self.sopener, self.sjoin("undo"))
                util.rename(self.join("undo.dirstate"), self.join("dirstate"))
                try:
                    branch = self.opener("undo.branch").read()
                    self.dirstate.setbranch(branch)
                except IOError:
                    self.ui.warn(_("Named branch could not be reset, "
                                   "current branch still is: %s\n")
                                 % util.tolocal(self.dirstate.branch()))
                self.invalidate()
                self.dirstate.invalidate()
            else:
                self.ui.warn(_("no rollback information available\n"))
        finally:
            del lock, wlock

    def invalidate(self):
        for a in "changelog manifest".split():
            if a in self.__dict__:
                delattr(self, a)
        self.tagscache = None
        self._tagstypecache = None
        self.nodetagscache = None
        self.branchcache = None
        self._ubranchcache = None
        self._branchcachetip = None

    def _lock(self, lockname, wait, releasefn, acquirefn, desc):
        try:
            l = lock.lock(lockname, 0, releasefn, desc=desc)
        except lock.LockHeld, inst:
            if not wait:
                raise
            self.ui.warn(_("waiting for lock on %s held by %r\n") %
                         (desc, inst.locker))
            # default to 600 seconds timeout
            l = lock.lock(lockname, int(self.ui.config("ui", "timeout", "600")),
                          releasefn, desc=desc)
        if acquirefn:
            acquirefn()
        return l

    def lock(self, wait=True):
        if self._lockref and self._lockref():
            return self._lockref()

        l = self._lock(self.sjoin("lock"), wait, None, self.invalidate,
                       _('repository %s') % self.origroot)
        self._lockref = weakref.ref(l)
        return l

    def wlock(self, wait=True):
        if self._wlockref and self._wlockref():
            return self._wlockref()

        l = self._lock(self.join("wlock"), wait, self.dirstate.write,
                       self.dirstate.invalidate, _('working directory of %s') %
                       self.origroot)
        self._wlockref = weakref.ref(l)
        return l

    def filecommit(self, fn, manifest1, manifest2, linkrev, tr, changelist):
        """
        commit an individual file as part of a larger transaction
        """

        t = self.wread(fn)
        fl = self.file(fn)
        fp1 = manifest1.get(fn, nullid)
        fp2 = manifest2.get(fn, nullid)

        meta = {}
        cp = self.dirstate.copied(fn)
        if cp:
            # Mark the new revision of this file as a copy of another
            # file.  This copy data will effectively act as a parent
            # of this new revision.  If this is a merge, the first
            # parent will be the nullid (meaning "look up the copy data")
            # and the second one will be the other parent.  For example:
            #
            # 0 --- 1 --- 3   rev1 changes file foo
            #   \       /     rev2 renames foo to bar and changes it
            #    \- 2 -/      rev3 should have bar with all changes and
            #                      should record that bar descends from
            #                      bar in rev2 and foo in rev1
            #
            # this allows this merge to succeed:
            #
            # 0 --- 1 --- 3   rev4 reverts the content change from rev2
            #   \       /     merging rev3 and rev4 should use bar@rev2
            #    \- 2 --- 4        as the merge base
            #
            meta["copy"] = cp
            if not manifest2: # not a branch merge
                meta["copyrev"] = hex(manifest1[cp])
                fp2 = nullid
            elif fp2 != nullid: # copied on remote side
                meta["copyrev"] = hex(manifest1[cp])
            elif fp1 != nullid: # copied on local side, reversed
                meta["copyrev"] = hex(manifest2[cp])
                fp2 = fp1
            elif cp in manifest2: # directory rename on local side
                meta["copyrev"] = hex(manifest2[cp])
            else: # directory rename on remote side
                meta["copyrev"] = hex(manifest1[cp])
            self.ui.debug(_(" %s: copy %s:%s\n") %
                          (fn, cp, meta["copyrev"]))
            fp1 = nullid
        elif fp2 != nullid:
            # is one parent an ancestor of the other?
            fpa = fl.ancestor(fp1, fp2)
            if fpa == fp1:
                fp1, fp2 = fp2, nullid
            elif fpa == fp2:
                fp2 = nullid

        # is the file unmodified from the parent? report existing entry
        if fp2 == nullid and not fl.cmp(fp1, t) and not meta:
            return fp1

        changelist.append(fn)
        return fl.add(t, meta, tr, linkrev, fp1, fp2)

    def rawcommit(self, files, text, user, date, p1=None, p2=None, extra={}):
        if p1 is None:
            p1, p2 = self.dirstate.parents()
        return self.commit(files=files, text=text, user=user, date=date,
                           p1=p1, p2=p2, extra=extra, empty_ok=True)

    def commit(self, files=None, text="", user=None, date=None,
               match=util.always, force=False, force_editor=False,
               p1=None, p2=None, extra={}, empty_ok=False):
        wlock = lock = tr = None
        valid = 0 # don't save the dirstate if this isn't set
        if files:
            files = util.unique(files)
        try:
            wlock = self.wlock()
            lock = self.lock()
            commit = []
            remove = []
            changed = []
            use_dirstate = (p1 is None) # not rawcommit
            extra = extra.copy()

            if use_dirstate:
                if files:
                    for f in files:
                        s = self.dirstate[f]
                        if s in 'nma':
                            commit.append(f)
                        elif s == 'r':
                            remove.append(f)
                        else:
                            self.ui.warn(_("%s not tracked!\n") % f)
                else:
                    changes = self.status(match=match)[:5]
                    modified, added, removed, deleted, unknown = changes
                    commit = modified + added
                    remove = removed
            else:
                commit = files

            if use_dirstate:
                p1, p2 = self.dirstate.parents()
                update_dirstate = True

                if (not force and p2 != nullid and
                    (files or match != util.always)):
                    raise util.Abort(_('cannot partially commit a merge '
                                       '(do not specify files or patterns)'))
            else:
                p1, p2 = p1, p2 or nullid
                update_dirstate = (self.dirstate.parents()[0] == p1)

            c1 = self.changelog.read(p1)
            c2 = self.changelog.read(p2)
            m1 = self.manifest.read(c1[0]).copy()
            m2 = self.manifest.read(c2[0])

            if use_dirstate:
                branchname = self.workingctx().branch()
                try:
                    branchname = branchname.decode('UTF-8').encode('UTF-8')
                except UnicodeDecodeError:
                    raise util.Abort(_('branch name not in UTF-8!'))
            else:
                branchname = ""

            if use_dirstate:
                oldname = c1[5].get("branch") # stored in UTF-8
                if (not commit and not remove and not force and p2 == nullid
                    and branchname == oldname):
                    self.ui.status(_("nothing changed\n"))
                    return None

            xp1 = hex(p1)
            if p2 == nullid: xp2 = ''
            else: xp2 = hex(p2)

            self.hook("precommit", throw=True, parent1=xp1, parent2=xp2)

            tr = self.transaction()
            trp = weakref.proxy(tr)

            # check in files
            new = {}
            linkrev = self.changelog.count()
            commit.sort()
            is_exec = util.execfunc(self.root, m1.execf)
            is_link = util.linkfunc(self.root, m1.linkf)
            for f in commit:
                self.ui.note(f + "\n")
                try:
                    new[f] = self.filecommit(f, m1, m2, linkrev, trp, changed)
                    new_exec = is_exec(f)
                    new_link = is_link(f)
                    if ((not changed or changed[-1] != f) and
                        m2.get(f) != new[f]):
                        # mention the file in the changelog if some
                        # flag changed, even if there was no content
                        # change.
                        old_exec = m1.execf(f)
                        old_link = m1.linkf(f)
                        if old_exec != new_exec or old_link != new_link:
                            changed.append(f)
                    m1.set(f, new_exec, new_link)
                    if use_dirstate:
                        self.dirstate.normal(f)

                except (OSError, IOError):
                    if use_dirstate:
                        self.ui.warn(_("trouble committing %s!\n") % f)
                        raise
                    else:
                        remove.append(f)

            # update manifest
            m1.update(new)
            remove.sort()
            removed = []

            for f in remove:
                if f in m1:
                    del m1[f]
                    removed.append(f)
                elif f in m2:
                    removed.append(f)
            mn = self.manifest.add(m1, trp, linkrev, c1[0], c2[0],
                                   (new, removed))

            # add changeset
            new = new.keys()
            new.sort()

            user = user or self.ui.username()
            if (not empty_ok and not text) or force_editor:
                edittext = []
                if text:
                    edittext.append(text)
                edittext.append("")
                edittext.append(_("HG: Enter commit message."
                                  "  Lines beginning with 'HG:' are removed."))
                edittext.append("HG: --")
                edittext.append("HG: user: %s" % user)
                if p2 != nullid:
                    edittext.append("HG: branch merge")
                if branchname:
                    edittext.append("HG: branch '%s'" % util.tolocal(branchname))
                edittext.extend(["HG: changed %s" % f for f in changed])
                edittext.extend(["HG: removed %s" % f for f in removed])
                if not changed and not remove:
                    edittext.append("HG: no files changed")
                edittext.append("")
                # run editor in the repository root
                olddir = os.getcwd()
                os.chdir(self.root)
                text = self.ui.edit("\n".join(edittext), user)
                os.chdir(olddir)

            if branchname:
                extra["branch"] = branchname

            lines = [line.rstrip() for line in text.rstrip().splitlines()]
            while lines and not lines[0]:
                del lines[0]
            if not lines and use_dirstate:
                raise util.Abort(_("empty commit message"))
            text = '\n'.join(lines)

            n = self.changelog.add(mn, changed + removed, text, trp, p1, p2,
                                   user, date, extra)
            self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                      parent2=xp2)
            tr.close()

            if self.branchcache:
                self.branchtags()

            if use_dirstate or update_dirstate:
                self.dirstate.setparents(n)
                if use_dirstate:
                    for f in removed:
                        self.dirstate.forget(f)
            valid = 1 # our dirstate updates are complete

            self.hook("commit", node=hex(n), parent1=xp1, parent2=xp2)
            return n
        finally:
            if not valid: # don't save our updated dirstate
                self.dirstate.invalidate()
            del tr, lock, wlock

    def walk(self, node=None, files=[], match=util.always, badmatch=None):
        '''
        walk recursively through the directory tree or a given
        changeset, finding all files matched by the match
        function

        results are yielded in a tuple (src, filename), where src
        is one of:
        'f' the file was found in the directory tree
        'm' the file was only in the dirstate and not in the tree
        'b' file was not found and matched badmatch
        '''

        if node:
            fdict = dict.fromkeys(files)
            # for dirstate.walk, files=['.'] means "walk the whole tree".
            # follow that here, too
            fdict.pop('.', None)
            mdict = self.manifest.read(self.changelog.read(node)[0])
            mfiles = mdict.keys()
            mfiles.sort()
            for fn in mfiles:
                for ffn in fdict:
                    # match if the file is the exact name or a directory
                    if ffn == fn or fn.startswith("%s/" % ffn):
                        del fdict[ffn]
                        break
                if match(fn):
                    yield 'm', fn
            ffiles = fdict.keys()
            ffiles.sort()
            for fn in ffiles:
                if badmatch and badmatch(fn):
                    if match(fn):
                        yield 'b', fn
                else:
                    self.ui.warn(_('%s: No such file in rev %s\n')
                                 % (self.pathto(fn), short(node)))
        else:
            for src, fn in self.dirstate.walk(files, match, badmatch=badmatch):
                yield src, fn

    def status(self, node1=None, node2=None, files=[], match=util.always,
               list_ignored=False, list_clean=False, list_unknown=True):
        """return status of files between two nodes or node and working directory

        If node1 is None, use the first dirstate parent instead.
        If node2 is None, compare node1 with working directory.
        """

        def fcmp(fn, getnode):
            t1 = self.wread(fn)
            return self.file(fn).cmp(getnode(fn), t1)

        def mfmatches(node):
            change = self.changelog.read(node)
            mf = self.manifest.read(change[0]).copy()
            for fn in mf.keys():
                if not match(fn):
                    del mf[fn]
            return mf

        modified, added, removed, deleted, unknown = [], [], [], [], []
        ignored, clean = [], []

        compareworking = False
        if not node1 or (not node2 and node1 == self.dirstate.parents()[0]):
            compareworking = True

        if not compareworking:
            # read the manifest from node1 before the manifest from node2,
            # so that we'll hit the manifest cache if we're going through
            # all the revisions in parent->child order.
            mf1 = mfmatches(node1)

        # are we comparing the working directory?
        if not node2:
            (lookup, modified, added, removed, deleted, unknown,
             ignored, clean) = self.dirstate.status(files, match,
                                                    list_ignored, list_clean,
                                                    list_unknown)

            # are we comparing working dir against its parent?
            if compareworking:
                if lookup:
                    fixup = []
                    # do a full compare of any files that might have changed
                    ctx = self.changectx()
                    mexec = lambda f: 'x' in ctx.fileflags(f)
                    mlink = lambda f: 'l' in ctx.fileflags(f)
                    is_exec = util.execfunc(self.root, mexec)
                    is_link = util.linkfunc(self.root, mlink)
                    def flags(f):
                        return is_link(f) and 'l' or is_exec(f) and 'x' or ''
                    for f in lookup:
                        if (f not in ctx or flags(f) != ctx.fileflags(f)
                            or ctx[f].cmp(self.wread(f))):
                            modified.append(f)
                        else:
                            fixup.append(f)
                            if list_clean:
                                clean.append(f)

                    # update dirstate for files that are actually clean
                    if fixup:
                        wlock = None
                        try:
                            try:
                                wlock = self.wlock(False)
                            except lock.LockException:
                                pass
                            if wlock:
                                for f in fixup:
                                    self.dirstate.normal(f)
                        finally:
                            del wlock
            else:
                # we are comparing working dir against non-parent
                # generate a pseudo-manifest for the working dir
                # XXX: create it in dirstate.py ?
                mf2 = mfmatches(self.dirstate.parents()[0])
                is_exec = util.execfunc(self.root, mf2.execf)
                is_link = util.linkfunc(self.root, mf2.linkf)
                for f in lookup + modified + added:
                    mf2[f] = ""
                    mf2.set(f, is_exec(f), is_link(f))
                for f in removed:
                    if f in mf2:
                        del mf2[f]

        else:
            # we are comparing two revisions
            mf2 = mfmatches(node2)

        if not compareworking:
            # flush lists from dirstate before comparing manifests
            modified, added, clean = [], [], []

            # make sure to sort the files so we talk to the disk in a
            # reasonable order
            mf2keys = mf2.keys()
            mf2keys.sort()
            getnode = lambda fn: mf1.get(fn, nullid)
            for fn in mf2keys:
                if fn in mf1:
                    if (mf1.flags(fn) != mf2.flags(fn) or
                        (mf1[fn] != mf2[fn] and
                         (mf2[fn] != "" or fcmp(fn, getnode)))):
                        modified.append(fn)
                    elif list_clean:
                        clean.append(fn)
                    del mf1[fn]
                else:
                    added.append(fn)

            removed = mf1.keys()

        # sort and return results:
        for l in modified, added, removed, deleted, unknown, ignored, clean:
            l.sort()
        return (modified, added, removed, deleted, unknown, ignored, clean)

    def add(self, list):
        wlock = self.wlock()
        try:
            rejected = []
            for f in list:
                p = self.wjoin(f)
                try:
                    st = os.lstat(p)
                except:
                    self.ui.warn(_("%s does not exist!\n") % f)
                    rejected.append(f)
                    continue
                if st.st_size > 10000000:
                    self.ui.warn(_("%s: files over 10MB may cause memory and"
                                   " performance problems\n"
                                   "(use 'hg revert %s' to unadd the file)\n")
                                   % (f, f))
                if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
                    self.ui.warn(_("%s not added: only files and symlinks "
                                   "supported currently\n") % f)
                    rejected.append(p)
                elif self.dirstate[f] in 'amn':
                    self.ui.warn(_("%s already tracked!\n") % f)
                elif self.dirstate[f] == 'r':
                    self.dirstate.normallookup(f)
                else:
                    self.dirstate.add(f)
            return rejected
        finally:
            del wlock

    def forget(self, list):
        wlock = self.wlock()
        try:
            for f in list:
                if self.dirstate[f] != 'a':
                    self.ui.warn(_("%s not added!\n") % f)
                else:
                    self.dirstate.forget(f)
        finally:
            del wlock

    def remove(self, list, unlink=False):
        wlock = None
        try:
            if unlink:
                for f in list:
                    try:
                        util.unlink(self.wjoin(f))
                    except OSError, inst:
                        if inst.errno != errno.ENOENT:
                            raise
            wlock = self.wlock()
            for f in list:
                if unlink and os.path.exists(self.wjoin(f)):
                    self.ui.warn(_("%s still exists!\n") % f)
                elif self.dirstate[f] == 'a':
                    self.dirstate.forget(f)
                elif f not in self.dirstate:
                    self.ui.warn(_("%s not tracked!\n") % f)
                else:
                    self.dirstate.remove(f)
        finally:
            del wlock

    def undelete(self, list):
        wlock = None
        try:
            manifests = [self.manifest.read(self.changelog.read(p)[0])
                         for p in self.dirstate.parents() if p != nullid]
            wlock = self.wlock()
            for f in list:
                if self.dirstate[f] != 'r':
                    self.ui.warn("%s not removed!\n" % f)
                else:
                    m = f in manifests[0] and manifests[0] or manifests[1]
                    t = self.file(f).read(m[f])
                    self.wwrite(f, t, m.flags(f))
                    self.dirstate.normal(f)
        finally:
            del wlock

    def copy(self, source, dest):
        wlock = None
        try:
            p = self.wjoin(dest)
            if not (os.path.exists(p) or os.path.islink(p)):
                self.ui.warn(_("%s does not exist!\n") % dest)
            elif not (os.path.isfile(p) or os.path.islink(p)):
                self.ui.warn(_("copy failed: %s is not a file or a "
                               "symbolic link\n") % dest)
            else:
                wlock = self.wlock()
                if dest not in self.dirstate:
                    self.dirstate.add(dest)
                self.dirstate.copy(source, dest)
        finally:
            del wlock

    def heads(self, start=None):
        heads = self.changelog.heads(start)
        # sort the output in rev descending order
        heads = [(-self.changelog.rev(h), h) for h in heads]
        heads.sort()
        return [n for (r, n) in heads]

    def branchheads(self, branch, start=None):
        branches = self.branchtags()
        if branch not in branches:
            return []
        # The basic algorithm is this:
        #
        # Start from the branch tip since there are no later revisions that can
        # possibly be in this branch, and the tip is a guaranteed head.
        #
        # Remember the tip's parents as the first ancestors, since these by
        # definition are not heads.
        #
        # Step backwards from the brach tip through all the revisions. We are
        # guaranteed by the rules of Mercurial that we will now be visiting the
        # nodes in reverse topological order (children before parents).
        #
        # If a revision is one of the ancestors of a head then we can toss it
        # out of the ancestors set (we've already found it and won't be
        # visiting it again) and put its parents in the ancestors set.
        #
        # Otherwise, if a revision is in the branch it's another head, since it
        # wasn't in the ancestor list of an existing head.  So add it to the
        # head list, and add its parents to the ancestor list.
        #
        # If it is not in the branch ignore it.
        #
        # Once we have a list of heads, use nodesbetween to filter out all the
        # heads that cannot be reached from startrev.  There may be a more
        # efficient way to do this as part of the previous algorithm.

        set = util.set
        heads = [self.changelog.rev(branches[branch])]
        # Don't care if ancestors contains nullrev or not.
        ancestors = set(self.changelog.parentrevs(heads[0]))
        for rev in xrange(heads[0] - 1, nullrev, -1):
            if rev in ancestors:
                ancestors.update(self.changelog.parentrevs(rev))
                ancestors.remove(rev)
            elif self.changectx(rev).branch() == branch:
                heads.append(rev)
                ancestors.update(self.changelog.parentrevs(rev))
        heads = [self.changelog.node(rev) for rev in heads]
        if start is not None:
            heads = self.changelog.nodesbetween([start], heads)[2]
        return heads

    def branches(self, nodes):
        if not nodes:
            nodes = [self.changelog.tip()]
        b = []
        for n in nodes:
            t = n
            while 1:
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

    def findincoming(self, remote, base=None, heads=None, force=False):
        """Return list of roots of the subsets of missing nodes from remote

        If base dict is specified, assume that these nodes and their parents
        exist on the remote side and that no child of a node of base exists
        in both remote and self.
        Furthermore base will be updated to include the nodes that exists
        in self and remote but no children exists in self and remote.
        If a list of heads is specified, return only nodes which are heads
        or ancestors of these heads.

        All the ancestors of base are in self and in remote.
        All the descendants of the list returned are missing in self.
        (and so we know that the rest of the nodes are missing in remote, see
        outgoing)
        """
        m = self.changelog.nodemap
        search = []
        fetch = {}
        seen = {}
        seenbranch = {}
        if base == None:
            base = {}

        if not heads:
            heads = remote.heads()

        if self.changelog.tip() == nullid:
            base[nullid] = 1
            if heads != [nullid]:
                return [nullid]
            return []

        # assume we're closer to the tip than the root
        # and start by examining the heads
        self.ui.status(_("searching for changes\n"))

        unknown = []
        for h in heads:
            if h not in m:
                unknown.append(h)
            else:
                base[h] = 1

        if not unknown:
            return []

        req = dict.fromkeys(unknown)
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
                if n[0] == nullid: # found the end of the branch
                    pass
                elif n in seenbranch:
                    self.ui.debug(_("branch already found\n"))
                    continue
                elif n[1] and n[1] in m: # do we know the base?
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
                        for p in n[2:4]:
                            if p in m:
                                base[p] = 1 # latest known

                    for p in n[2:4]:
                        if p not in req and p not in m:
                            r.append(p)
                            req[p] = 1
                seen[n[0]] = 1

            if r:
                reqcnt += 1
                self.ui.debug(_("request %d: %s\n") %
                            (reqcnt, " ".join(map(short, r))))
                for p in xrange(0, len(r), 10):
                    for b in remote.branches(r[p:p+10]):
                        self.ui.debug(_("received %s:%s\n") %
                                      (short(b[0]), short(b[1])))
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
            if force:
                self.ui.warn(_("warning: repository is unrelated\n"))
            else:
                raise util.Abort(_("repository is unrelated"))

        self.ui.debug(_("found new changesets starting at ") +
                     " ".join([short(f) for f in fetch]) + "\n")

        self.ui.debug(_("%d total queries\n") % reqcnt)

        return fetch.keys()

    def findoutgoing(self, remote, base=None, heads=None, force=False):
        """Return list of nodes that are roots of subsets not in remote

        If base dict is specified, assume that these nodes and their parents
        exist on the remote side.
        If a list of heads is specified, return only nodes which are heads
        or ancestors of these heads, and return a second element which
        contains all remote heads which get new children.
        """
        if base == None:
            base = {}
            self.findincoming(remote, base, heads, force=force)

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
        # find every remote head that will get new children
        updated_heads = {}
        for n in remain:
            p1, p2 = self.changelog.parents(n)
            if p1 not in remain and p2 not in remain:
                subset.append(n)
            if heads:
                if p1 in heads:
                    updated_heads[p1] = True
                if p2 in heads:
                    updated_heads[p2] = True

        # this is the set of all roots we have to push
        if heads:
            return subset, updated_heads.keys()
        else:
            return subset

    def pull(self, remote, heads=None, force=False):
        lock = self.lock()
        try:
            fetch = self.findincoming(remote, heads=heads, force=force)
            if fetch == [nullid]:
                self.ui.status(_("requesting all changes\n"))

            if not fetch:
                self.ui.status(_("no changes found\n"))
                return 0

            if heads is None:
                cg = remote.changegroup(fetch, 'pull')
            else:
                if 'changegroupsubset' not in remote.capabilities:
                    raise util.Abort(_("Partial pull cannot be done because other repository doesn't support changegroupsubset."))
                cg = remote.changegroupsubset(fetch, heads, 'pull')
            return self.addchangegroup(cg, 'pull', remote.url())
        finally:
            del lock

    def push(self, remote, force=False, revs=None):
        # there are two ways to push to remote repo:
        #
        # addchangegroup assumes local user can lock remote
        # repo (local filesystem, old ssh servers).
        #
        # unbundle assumes local user cannot lock remote repo (new ssh
        # servers, http servers).

        if remote.capable('unbundle'):
            return self.push_unbundle(remote, force, revs)
        return self.push_addchangegroup(remote, force, revs)

    def prepush(self, remote, force, revs):
        base = {}
        remote_heads = remote.heads()
        inc = self.findincoming(remote, base, remote_heads, force=force)

        update, updated_heads = self.findoutgoing(remote, base, remote_heads)
        if revs is not None:
            msng_cl, bases, heads = self.changelog.nodesbetween(update, revs)
        else:
            bases, heads = update, self.changelog.heads()

        if not bases:
            self.ui.status(_("no changes found\n"))
            return None, 1
        elif not force:
            # check if we're creating new remote heads
            # to be a remote head after push, node must be either
            # - unknown locally
            # - a local outgoing head descended from update
            # - a remote head that's known locally and not
            #   ancestral to an outgoing head

            warn = 0

            if remote_heads == [nullid]:
                warn = 0
            elif not revs and len(heads) > len(remote_heads):
                warn = 1
            else:
                newheads = list(heads)
                for r in remote_heads:
                    if r in self.changelog.nodemap:
                        desc = self.changelog.heads(r, heads)
                        l = [h for h in heads if h in desc]
                        if not l:
                            newheads.append(r)
                    else:
                        newheads.append(r)
                if len(newheads) > len(remote_heads):
                    warn = 1

            if warn:
                self.ui.warn(_("abort: push creates new remote heads!\n"))
                self.ui.status(_("(did you forget to merge?"
                                 " use push -f to force)\n"))
                return None, 0
            elif inc:
                self.ui.warn(_("note: unsynced remote changes!\n"))


        if revs is None:
            cg = self.changegroup(update, 'push')
        else:
            cg = self.changegroupsubset(update, revs, 'push')
        return cg, remote_heads

    def push_addchangegroup(self, remote, force, revs):
        lock = remote.lock()
        try:
            ret = self.prepush(remote, force, revs)
            if ret[0] is not None:
                cg, remote_heads = ret
                return remote.addchangegroup(cg, 'push', self.url())
            return ret[1]
        finally:
            del lock

    def push_unbundle(self, remote, force, revs):
        # local repo finds heads on server, finds out what revs it
        # must push.  once revs transferred, if server finds it has
        # different heads (someone else won commit/push race), server
        # aborts.

        ret = self.prepush(remote, force, revs)
        if ret[0] is not None:
            cg, remote_heads = ret
            if force: remote_heads = ['force']
            return remote.unbundle(cg, remote_heads, 'push')
        return ret[1]

    def changegroupinfo(self, nodes, source):
        if self.ui.verbose or source == 'bundle':
            self.ui.status(_("%d changesets found\n") % len(nodes))
        if self.ui.debugflag:
            self.ui.debug(_("List of changesets:\n"))
            for node in nodes:
                self.ui.debug("%s\n" % hex(node))

    def changegroupsubset(self, bases, heads, source, extranodes=None):
        """This function generates a changegroup consisting of all the nodes
        that are descendents of any of the bases, and ancestors of any of
        the heads.

        It is fairly complex as determining which filenodes and which
        manifest nodes need to be included for the changeset to be complete
        is non-trivial.

        Another wrinkle is doing the reverse, figuring out which changeset in
        the changegroup a particular filenode or manifestnode belongs to.

        The caller can specify some nodes that must be included in the
        changegroup using the extranodes argument.  It should be a dict
        where the keys are the filenames (or 1 for the manifest), and the
        values are lists of (node, linknode) tuples, where node is a wanted
        node and linknode is the changelog node that should be transmitted as
        the linkrev.
        """

        self.hook('preoutgoing', throw=True, source=source)

        # Set up some initial variables
        # Make it easy to refer to self.changelog
        cl = self.changelog
        # msng is short for missing - compute the list of changesets in this
        # changegroup.
        msng_cl_lst, bases, heads = cl.nodesbetween(bases, heads)
        self.changegroupinfo(msng_cl_lst, source)
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
                    deltamf = mnfst.readdelta(mnfstnode)
                    # For each line in the delta
                    for f, fnode in deltamf.items():
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

        # Add the nodes that were explicitly requested.
        def add_extra_nodes(name, nodes):
            if not extranodes or name not in extranodes:
                return

            for node, linknode in extranodes[name]:
                if node not in nodes:
                    nodes[node] = linknode

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
            add_extra_nodes(1, msng_mnfst_set)
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

            if extranodes:
                for fname in extranodes:
                    if isinstance(fname, int):
                        continue
                    add_extra_nodes(fname,
                                    msng_filenode_set.setdefault(fname, {}))
                    changedfiles[fname] = 1
            changedfiles = changedfiles.keys()
            changedfiles.sort()
            # Go through all our files in order sorted by name.
            for fname in changedfiles:
                filerevlog = self.file(fname)
                if filerevlog.count() == 0:
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                # Toss out the filenodes that the recipient isn't really
                # missing.
                if fname in msng_filenode_set:
                    prune_filenodes(fname, filerevlog)
                    msng_filenode_lst = msng_filenode_set[fname].keys()
                else:
                    msng_filenode_lst = []
                # If any filenodes are left, generate the group for them,
                # otherwise don't bother.
                if len(msng_filenode_lst) > 0:
                    yield changegroup.chunkheader(len(fname))
                    yield fname
                    # Sort the filenodes by their revision #
                    msng_filenode_lst.sort(cmp_by_rev_func(filerevlog))
                    # Create a group generator and only pass in a changenode
                    # lookup function as we need to collect no information
                    # from filenodes.
                    group = filerevlog.group(msng_filenode_lst,
                                             lookup_filenode_link_func(fname))
                    for chnk in group:
                        yield chnk
                if fname in msng_filenode_set:
                    # Don't need this anymore, toss it to free memory.
                    del msng_filenode_set[fname]
            # Signal that no more groups are left.
            yield changegroup.closechunk()

            if msng_cl_lst:
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
        self.changegroupinfo(nodes, source)

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
                if filerevlog.count() == 0:
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                nodeiter = gennodelst(filerevlog)
                nodeiter = list(nodeiter)
                if nodeiter:
                    yield changegroup.chunkheader(len(fname))
                    yield fname
                    lookup = lookuprevlink_func(filerevlog)
                    for chnk in filerevlog.group(nodeiter, lookup):
                        yield chnk

            yield changegroup.closechunk()

            if nodes:
                self.hook('outgoing', node=hex(nodes[0]), source=source)

        return util.chunkbuffer(gengroup())

    def addchangegroup(self, source, srctype, url, emptyok=False):
        """add changegroup to repo.

        return values:
        - nothing changed or no source: 0
        - more heads than before: 1+added heads (2..n)
        - less heads than before: -1-removed heads (-2..-n)
        - number of heads stays the same: 1
        """
        def csmap(x):
            self.ui.debug(_("add changeset %s\n") % short(x))
            return cl.count()

        def revmap(x):
            return cl.rev(x)

        if not source:
            return 0

        self.hook('prechangegroup', throw=True, source=srctype, url=url)

        changesets = files = revisions = 0

        # write changelog data to temp files so concurrent readers will not see
        # inconsistent view
        cl = self.changelog
        cl.delayupdate()
        oldheads = len(cl.heads())

        tr = self.transaction()
        try:
            trp = weakref.proxy(tr)
            # pull off the changeset group
            self.ui.status(_("adding changesets\n"))
            cor = cl.count() - 1
            chunkiter = changegroup.chunkiter(source)
            if cl.addgroup(chunkiter, csmap, trp, 1) is None and not emptyok:
                raise util.Abort(_("received changelog group is empty"))
            cnr = cl.count() - 1
            changesets = cnr - cor

            # pull off the manifest group
            self.ui.status(_("adding manifests\n"))
            chunkiter = changegroup.chunkiter(source)
            # no need to check for empty manifest group here:
            # if the result of the merge of 1 and 2 is the same in 3 and 4,
            # no new manifest will be created and the manifest group will
            # be empty during the pull
            self.manifest.addgroup(chunkiter, revmap, trp)

            # process the files
            self.ui.status(_("adding file changes\n"))
            while 1:
                f = changegroup.getchunk(source)
                if not f:
                    break
                self.ui.debug(_("adding %s revisions\n") % f)
                fl = self.file(f)
                o = fl.count()
                chunkiter = changegroup.chunkiter(source)
                if fl.addgroup(chunkiter, revmap, trp) is None:
                    raise util.Abort(_("received file revlog group is empty"))
                revisions += fl.count() - o
                files += 1

            # make changelog see real files again
            cl.finalize(trp)

            newheads = len(self.changelog.heads())
            heads = ""
            if oldheads and newheads != oldheads:
                heads = _(" (%+d heads)") % (newheads - oldheads)

            self.ui.status(_("added %d changesets"
                             " with %d changes to %d files%s\n")
                             % (changesets, revisions, files, heads))

            if changesets > 0:
                self.hook('pretxnchangegroup', throw=True,
                          node=hex(self.changelog.node(cor+1)), source=srctype,
                          url=url)

            tr.close()
        finally:
            del tr

        if changesets > 0:
            # forcefully update the on-disk branch cache
            self.ui.debug(_("updating the branch cache\n"))
            self.branchtags()
            self.hook("changegroup", node=hex(self.changelog.node(cor+1)),
                      source=srctype, url=url)

            for i in xrange(cor + 1, cnr + 1):
                self.hook("incoming", node=hex(self.changelog.node(i)),
                          source=srctype, url=url)

        # never return 0 here:
        if newheads < oldheads:
            return newheads - oldheads - 1
        else:
            return newheads - oldheads + 1


    def stream_in(self, remote):
        fp = remote.stream_out()
        l = fp.readline()
        try:
            resp = int(l)
        except ValueError:
            raise util.UnexpectedOutput(
                _('Unexpected response from remote server:'), l)
        if resp == 1:
            raise util.Abort(_('operation forbidden by server'))
        elif resp == 2:
            raise util.Abort(_('locking the remote repository failed'))
        elif resp != 0:
            raise util.Abort(_('the server sent an unknown error code'))
        self.ui.status(_('streaming all changes\n'))
        l = fp.readline()
        try:
            total_files, total_bytes = map(int, l.split(' ', 1))
        except (ValueError, TypeError):
            raise util.UnexpectedOutput(
                _('Unexpected response from remote server:'), l)
        self.ui.status(_('%d files to transfer, %s of data\n') %
                       (total_files, util.bytecount(total_bytes)))
        start = time.time()
        for i in xrange(total_files):
            # XXX doesn't support '\n' or '\r' in filenames
            l = fp.readline()
            try:
                name, size = l.split('\0', 1)
                size = int(size)
            except ValueError, TypeError:
                raise util.UnexpectedOutput(
                    _('Unexpected response from remote server:'), l)
            self.ui.debug('adding %s (%s)\n' % (name, util.bytecount(size)))
            ofp = self.sopener(name, 'w')
            for chunk in util.filechunkiter(fp, limit=size):
                ofp.write(chunk)
            ofp.close()
        elapsed = time.time() - start
        if elapsed <= 0:
            elapsed = 0.001
        self.ui.status(_('transferred %s in %.1f seconds (%s/sec)\n') %
                       (util.bytecount(total_bytes), elapsed,
                        util.bytecount(total_bytes / elapsed)))
        self.invalidate()
        return len(self.heads()) + 1

    def clone(self, remote, heads=[], stream=False):
        '''clone remote repository.

        keyword arguments:
        heads: list of revs to clone (forces use of pull)
        stream: use streaming clone if possible'''

        # now, all clients that can request uncompressed clones can
        # read repo formats supported by all servers that can serve
        # them.

        # if revlog format changes, client will have to check version
        # and format flags on "stream" capability, and use
        # uncompressed only if compatible.

        if stream and not heads and remote.capable('stream'):
            return self.stream_in(remote)
        return self.pull(remote, heads)

# used to avoid circular references so destructors work
def aftertrans(files):
    renamefiles = [tuple(t) for t in files]
    def a():
        for src, dest in renamefiles:
            util.rename(src, dest)
    return a

def instance(ui, path, create):
    return localrepository(ui, util.drop_scheme('file', path), create)

def islocal(path):
    return True
