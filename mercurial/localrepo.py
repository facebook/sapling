# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev, short
from i18n import _
import repo, changegroup, subrepo, discovery, pushkey
import changelog, dirstate, filelog, manifest, context, bookmarks
import lock, transaction, store, encoding
import util, extensions, hook, error
import match as matchmod
import merge as mergemod
import tags as tagsmod
import url as urlmod
from lock import release
import weakref, errno, os, time, inspect
propertycache = util.propertycache

class localrepository(repo.repository):
    capabilities = set(('lookup', 'changegroupsubset', 'branchmap', 'pushkey'))
    supportedformats = set(('revlogv1', 'parentdelta'))
    supported = supportedformats | set(('store', 'fncache', 'shared',
                                        'dotencode'))

    def __init__(self, baseui, path=None, create=0):
        repo.repository.__init__(self)
        self.root = os.path.realpath(util.expandpath(path))
        self.path = os.path.join(self.root, ".hg")
        self.origroot = path
        self.auditor = util.path_auditor(self.root, self._checknested)
        self.opener = util.opener(self.path)
        self.wopener = util.opener(self.root)
        self.baseui = baseui
        self.ui = baseui.copy()

        try:
            self.ui.readconfig(self.join("hgrc"), self.root)
            extensions.loadall(self.ui)
        except IOError:
            pass

        if not os.path.isdir(self.path):
            if create:
                if not os.path.exists(path):
                    util.makedirs(path)
                os.mkdir(self.path)
                requirements = ["revlogv1"]
                if self.ui.configbool('format', 'usestore', True):
                    os.mkdir(os.path.join(self.path, "store"))
                    requirements.append("store")
                    if self.ui.configbool('format', 'usefncache', True):
                        requirements.append("fncache")
                        if self.ui.configbool('format', 'dotencode', True):
                            requirements.append('dotencode')
                    # create an invalid changelog
                    self.opener("00changelog.i", "a").write(
                        '\0\0\0\2' # represents revlogv2
                        ' dummy changelog to prevent using the old repo layout'
                    )
                if self.ui.configbool('format', 'parentdelta', False):
                    requirements.append("parentdelta")
            else:
                raise error.RepoError(_("repository %s not found") % path)
        elif create:
            raise error.RepoError(_("repository %s already exists") % path)
        else:
            # find requirements
            requirements = set()
            try:
                requirements = set(self.opener("requires").read().splitlines())
            except IOError, inst:
                if inst.errno != errno.ENOENT:
                    raise
            for r in requirements - self.supported:
                raise error.RequirementError(
                          _("requirement '%s' not supported") % r)

        self.sharedpath = self.path
        try:
            s = os.path.realpath(self.opener("sharedpath").read())
            if not os.path.exists(s):
                raise error.RepoError(
                    _('.hg/sharedpath points to nonexistent directory %s') % s)
            self.sharedpath = s
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise

        self.store = store.store(requirements, self.sharedpath, util.opener)
        self.spath = self.store.path
        self.sopener = self.store.opener
        self.sjoin = self.store.join
        self.opener.createmode = self.store.createmode
        self._applyrequirements(requirements)
        if create:
            self._writerequirements()

        # These two define the set of tags for this repository.  _tags
        # maps tag name to node; _tagtypes maps tag name to 'global' or
        # 'local'.  (Global tags are defined by .hgtags across all
        # heads, and local tags are defined in .hg/localtags.)  They
        # constitute the in-memory cache of tags.
        self._tags = None
        self._tagtypes = None

        self._branchcache = None
        self._branchcachetip = None
        self.nodetagscache = None
        self.filterpats = {}
        self._datafilters = {}
        self._transref = self._lockref = self._wlockref = None

    def _applyrequirements(self, requirements):
        self.requirements = requirements
        self.sopener.options = {}
        if 'parentdelta' in requirements:
            self.sopener.options['parentdelta'] = 1

    def _writerequirements(self):
        reqfile = self.opener("requires", "w")
        for r in self.requirements:
            reqfile.write("%s\n" % r)
        reqfile.close()

    def _checknested(self, path):
        """Determine if path is a legal nested repository."""
        if not path.startswith(self.root):
            return False
        subpath = path[len(self.root) + 1:]

        # XXX: Checking against the current working copy is wrong in
        # the sense that it can reject things like
        #
        #   $ hg cat -r 10 sub/x.txt
        #
        # if sub/ is no longer a subrepository in the working copy
        # parent revision.
        #
        # However, it can of course also allow things that would have
        # been rejected before, such as the above cat command if sub/
        # is a subrepository now, but was a normal directory before.
        # The old path auditor would have rejected by mistake since it
        # panics when it sees sub/.hg/.
        #
        # All in all, checking against the working copy seems sensible
        # since we want to prevent access to nested repositories on
        # the filesystem *now*.
        ctx = self[None]
        parts = util.splitpath(subpath)
        while parts:
            prefix = os.sep.join(parts)
            if prefix in ctx.substate:
                if prefix == subpath:
                    return True
                else:
                    sub = ctx.sub(prefix)
                    return sub.checknested(subpath[len(prefix) + 1:])
            else:
                parts.pop()
        return False

    @util.propertycache
    def _bookmarks(self):
        return bookmarks.read(self)

    @util.propertycache
    def _bookmarkcurrent(self):
        return bookmarks.readcurrent(self)

    @propertycache
    def changelog(self):
        c = changelog.changelog(self.sopener)
        if 'HG_PENDING' in os.environ:
            p = os.environ['HG_PENDING']
            if p.startswith(self.root):
                c.readpending('00changelog.i.a')
        self.sopener.options['defversion'] = c.version
        return c

    @propertycache
    def manifest(self):
        return manifest.manifest(self.sopener)

    @propertycache
    def dirstate(self):
        warned = [0]
        def validate(node):
            try:
                r = self.changelog.rev(node)
                return node
            except error.LookupError:
                if not warned[0]:
                    warned[0] = True
                    self.ui.warn(_("warning: ignoring unknown"
                                   " working parent %s!\n") % short(node))
                return nullid

        return dirstate.dirstate(self.opener, self.ui, self.root, validate)

    def __getitem__(self, changeid):
        if changeid is None:
            return context.workingctx(self)
        return context.changectx(self, changeid)

    def __contains__(self, changeid):
        try:
            return bool(self.lookup(changeid))
        except error.RepoLookupError:
            return False

    def __nonzero__(self):
        return True

    def __len__(self):
        return len(self.changelog)

    def __iter__(self):
        for i in xrange(len(self)):
            yield i

    def url(self):
        return 'file:' + self.root

    def hook(self, name, throw=False, **args):
        return hook.hook(self.ui, self, name, throw, **args)

    tag_disallowed = ':\r\n'

    def _tag(self, names, node, message, local, user, date, extra={}):
        if isinstance(names, str):
            allchars = names
            names = (names,)
        else:
            allchars = ''.join(names)
        for c in self.tag_disallowed:
            if c in allchars:
                raise util.Abort(_('%r cannot be used in a tag name') % c)

        branches = self.branchmap()
        for name in names:
            self.hook('pretag', throw=True, node=hex(node), tag=name,
                      local=local)
            if name in branches:
                self.ui.warn(_("warning: tag %s conflicts with existing"
                " branch name\n") % name)

        def writetags(fp, names, munge, prevtags):
            fp.seek(0, 2)
            if prevtags and prevtags[-1] != '\n':
                fp.write('\n')
            for name in names:
                m = munge and munge(name) or name
                if self._tagtypes and name in self._tagtypes:
                    old = self._tags.get(name, nullid)
                    fp.write('%s %s\n' % (hex(old), m))
                fp.write('%s %s\n' % (hex(node), m))
            fp.close()

        prevtags = ''
        if local:
            try:
                fp = self.opener('localtags', 'r+')
            except IOError:
                fp = self.opener('localtags', 'a')
            else:
                prevtags = fp.read()

            # local tags are stored in the current charset
            writetags(fp, names, None, prevtags)
            for name in names:
                self.hook('tag', node=hex(node), tag=name, local=local)
            return

        try:
            fp = self.wfile('.hgtags', 'rb+')
        except IOError:
            fp = self.wfile('.hgtags', 'ab')
        else:
            prevtags = fp.read()

        # committed tags are stored in UTF-8
        writetags(fp, names, encoding.fromlocal, prevtags)

        fp.close()

        if '.hgtags' not in self.dirstate:
            self[None].add(['.hgtags'])

        m = matchmod.exact(self.root, '', ['.hgtags'])
        tagnode = self.commit(message, user, date, extra=extra, match=m)

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

        if not local:
            for x in self.status()[:5]:
                if '.hgtags' in x:
                    raise util.Abort(_('working copy of .hgtags is changed '
                                       '(please commit .hgtags manually)'))

        self.tags() # instantiate the cache
        self._tag(names, node, message, local, user, date)

    def tags(self):
        '''return a mapping of tag to node'''
        if self._tags is None:
            (self._tags, self._tagtypes) = self._findtags()

        return self._tags

    def _findtags(self):
        '''Do the hard work of finding tags.  Return a pair of dicts
        (tags, tagtypes) where tags maps tag name to node, and tagtypes
        maps tag name to a string like \'global\' or \'local\'.
        Subclasses or extensions are free to add their own tags, but
        should be aware that the returned dicts will be retained for the
        duration of the localrepo object.'''

        # XXX what tagtype should subclasses/extensions use?  Currently
        # mq and bookmarks add tags, but do not set the tagtype at all.
        # Should each extension invent its own tag type?  Should there
        # be one tagtype for all such "virtual" tags?  Or is the status
        # quo fine?

        alltags = {}                    # map tag name to (node, hist)
        tagtypes = {}

        tagsmod.findglobaltags(self.ui, self, alltags, tagtypes)
        tagsmod.readlocaltags(self.ui, self, alltags, tagtypes)

        # Build the return dicts.  Have to re-encode tag names because
        # the tags module always uses UTF-8 (in order not to lose info
        # writing to the cache), but the rest of Mercurial wants them in
        # local encoding.
        tags = {}
        for (name, (node, hist)) in alltags.iteritems():
            if node != nullid:
                tags[encoding.tolocal(name)] = node
        tags['tip'] = self.changelog.tip()
        tagtypes = dict([(encoding.tolocal(name), value)
                         for (name, value) in tagtypes.iteritems()])
        return (tags, tagtypes)

    def tagtype(self, tagname):
        '''
        return the type of the given tag. result can be:

        'local'  : a local tag
        'global' : a global tag
        None     : tag does not exist
        '''

        self.tags()

        return self._tagtypes.get(tagname)

    def tagslist(self):
        '''return a list of tags ordered by revision'''
        l = []
        for t, n in self.tags().iteritems():
            try:
                r = self.changelog.rev(n)
            except:
                r = -2 # sort to the beginning of the list if unknown
            l.append((r, t, n))
        return [(t, n) for r, t, n in sorted(l)]

    def nodetags(self, node):
        '''return the tags associated with a node'''
        if not self.nodetagscache:
            self.nodetagscache = {}
            for t, n in self.tags().iteritems():
                self.nodetagscache.setdefault(n, []).append(t)
            for tags in self.nodetagscache.itervalues():
                tags.sort()
        return self.nodetagscache.get(node, [])

    def nodebookmarks(self, node):
        marks = []
        for bookmark, n in self._bookmarks.iteritems():
            if n == node:
                marks.append(bookmark)
        return sorted(marks)

    def _branchtags(self, partial, lrev):
        # TODO: rename this function?
        tiprev = len(self) - 1
        if lrev != tiprev:
            ctxgen = (self[r] for r in xrange(lrev + 1, tiprev + 1))
            self._updatebranchcache(partial, ctxgen)
            self._writebranchcache(partial, self.changelog.tip(), tiprev)

        return partial

    def updatebranchcache(self):
        tip = self.changelog.tip()
        if self._branchcache is not None and self._branchcachetip == tip:
            return self._branchcache

        oldtip = self._branchcachetip
        self._branchcachetip = tip
        if oldtip is None or oldtip not in self.changelog.nodemap:
            partial, last, lrev = self._readbranchcache()
        else:
            lrev = self.changelog.rev(oldtip)
            partial = self._branchcache

        self._branchtags(partial, lrev)
        # this private cache holds all heads (not just tips)
        self._branchcache = partial

    def branchmap(self):
        '''returns a dictionary {branch: [branchheads]}'''
        self.updatebranchcache()
        return self._branchcache

    def branchtags(self):
        '''return a dict where branch names map to the tipmost head of
        the branch, open heads come before closed'''
        bt = {}
        for bn, heads in self.branchmap().iteritems():
            tip = heads[-1]
            for h in reversed(heads):
                if 'close' not in self.changelog.read(h)[5]:
                    tip = h
                    break
            bt[bn] = tip
        return bt

    def _readbranchcache(self):
        partial = {}
        try:
            f = self.opener("cache/branchheads")
            lines = f.read().split('\n')
            f.close()
        except (IOError, OSError):
            return {}, nullid, nullrev

        try:
            last, lrev = lines.pop(0).split(" ", 1)
            last, lrev = bin(last), int(lrev)
            if lrev >= len(self) or self[lrev].node() != last:
                # invalidate the cache
                raise ValueError('invalidating branch cache (tip differs)')
            for l in lines:
                if not l:
                    continue
                node, label = l.split(" ", 1)
                label = encoding.tolocal(label.strip())
                partial.setdefault(label, []).append(bin(node))
        except KeyboardInterrupt:
            raise
        except Exception, inst:
            if self.ui.debugflag:
                self.ui.warn(str(inst), '\n')
            partial, last, lrev = {}, nullid, nullrev
        return partial, last, lrev

    def _writebranchcache(self, branches, tip, tiprev):
        try:
            f = self.opener("cache/branchheads", "w", atomictemp=True)
            f.write("%s %s\n" % (hex(tip), tiprev))
            for label, nodes in branches.iteritems():
                for node in nodes:
                    f.write("%s %s\n" % (hex(node), encoding.fromlocal(label)))
            f.rename()
        except (IOError, OSError):
            pass

    def _updatebranchcache(self, partial, ctxgen):
        # collect new branch entries
        newbranches = {}
        for c in ctxgen:
            newbranches.setdefault(c.branch(), []).append(c.node())
        # if older branchheads are reachable from new ones, they aren't
        # really branchheads. Note checking parents is insufficient:
        # 1 (branch a) -> 2 (branch b) -> 3 (branch a)
        for branch, newnodes in newbranches.iteritems():
            bheads = partial.setdefault(branch, [])
            bheads.extend(newnodes)
            if len(bheads) <= 1:
                continue
            # starting from tip means fewer passes over reachable
            while newnodes:
                latest = newnodes.pop()
                if latest not in bheads:
                    continue
                minbhrev = self[min([self[bh].rev() for bh in bheads])].node()
                reachable = self.changelog.reachable(latest, minbhrev)
                reachable.remove(latest)
                bheads = [b for b in bheads if b not in reachable]
            partial[branch] = bheads

    def lookup(self, key):
        if isinstance(key, int):
            return self.changelog.node(key)
        elif key == '.':
            return self.dirstate.parents()[0]
        elif key == 'null':
            return nullid
        elif key == 'tip':
            return self.changelog.tip()
        n = self.changelog._match(key)
        if n:
            return n
        if key in self._bookmarks:
            return self._bookmarks[key]
        if key in self.tags():
            return self.tags()[key]
        if key in self.branchtags():
            return self.branchtags()[key]
        n = self.changelog._partialmatch(key)
        if n:
            return n

        # can't find key, check if it might have come from damaged dirstate
        if key in self.dirstate.parents():
            raise error.Abort(_("working directory has unknown parent '%s'!")
                              % short(key))
        try:
            if len(key) == 20:
                key = hex(key)
        except:
            pass
        raise error.RepoLookupError(_("unknown revision '%s'") % key)

    def lookupbranch(self, key, remote=None):
        repo = remote or self
        if key in repo.branchmap():
            return key

        repo = (remote and remote.local()) and remote or self
        return repo[key].branch()

    def local(self):
        return True

    def join(self, f):
        return os.path.join(self.path, f)

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def file(self, f):
        if f[0] == '/':
            f = f[1:]
        return filelog.filelog(self.sopener, f)

    def changectx(self, changeid):
        return self[changeid]

    def parents(self, changeid=None):
        '''get list of changectxs for parents of changeid'''
        return self[changeid].parents()

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

    def _loadfilter(self, filter):
        if filter not in self.filterpats:
            l = []
            for pat, cmd in self.ui.configitems(filter):
                if cmd == '!':
                    continue
                mf = matchmod.match(self.root, '', [pat])
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
        return self.filterpats[filter]

    def _filter(self, filterpats, filename, data):
        for mf, fn, cmd in filterpats:
            if mf(filename):
                self.ui.debug("filtering %s through %s\n" % (filename, cmd))
                data = fn(data, cmd, ui=self.ui, repo=self, filename=filename)
                break

        return data

    @propertycache
    def _encodefilterpats(self):
        return self._loadfilter('encode')

    @propertycache
    def _decodefilterpats(self):
        return self._loadfilter('decode')

    def adddatafilter(self, name, filter):
        self._datafilters[name] = filter

    def wread(self, filename):
        if self._link(filename):
            data = os.readlink(self.wjoin(filename))
        else:
            data = self.wopener(filename, 'r').read()
        return self._filter(self._encodefilterpats, filename, data)

    def wwrite(self, filename, data, flags):
        data = self._filter(self._decodefilterpats, filename, data)
        if 'l' in flags:
            self.wopener.symlink(data, filename)
        else:
            self.wopener(filename, 'w').write(data)
            if 'x' in flags:
                util.set_flags(self.wjoin(filename), False, True)

    def wwritedata(self, filename, data):
        return self._filter(self._decodefilterpats, filename, data)

    def transaction(self, desc):
        tr = self._transref and self._transref() or None
        if tr and tr.running():
            return tr.nest()

        # abort here if the journal already exists
        if os.path.exists(self.sjoin("journal")):
            raise error.RepoError(
                _("abandoned transaction found - run hg recover"))

        # save dirstate for rollback
        try:
            ds = self.opener("dirstate").read()
        except IOError:
            ds = ""
        self.opener("journal.dirstate", "w").write(ds)
        self.opener("journal.branch", "w").write(
            encoding.fromlocal(self.dirstate.branch()))
        self.opener("journal.desc", "w").write("%d\n%s\n" % (len(self), desc))

        renames = [(self.sjoin("journal"), self.sjoin("undo")),
                   (self.join("journal.dirstate"), self.join("undo.dirstate")),
                   (self.join("journal.branch"), self.join("undo.branch")),
                   (self.join("journal.desc"), self.join("undo.desc"))]
        tr = transaction.transaction(self.ui.warn, self.sopener,
                                     self.sjoin("journal"),
                                     aftertrans(renames),
                                     self.store.createmode)
        self._transref = weakref.ref(tr)
        return tr

    def recover(self):
        lock = self.lock()
        try:
            if os.path.exists(self.sjoin("journal")):
                self.ui.status(_("rolling back interrupted transaction\n"))
                transaction.rollback(self.sopener, self.sjoin("journal"),
                                     self.ui.warn)
                self.invalidate()
                return True
            else:
                self.ui.warn(_("no interrupted transaction available\n"))
                return False
        finally:
            lock.release()

    def rollback(self, dryrun=False):
        wlock = lock = None
        try:
            wlock = self.wlock()
            lock = self.lock()
            if os.path.exists(self.sjoin("undo")):
                try:
                    args = self.opener("undo.desc", "r").read().splitlines()
                    if len(args) >= 3 and self.ui.verbose:
                        desc = _("repository tip rolled back to revision %s"
                                 " (undo %s: %s)\n") % (
                                 int(args[0]) - 1, args[1], args[2])
                    elif len(args) >= 2:
                        desc = _("repository tip rolled back to revision %s"
                                 " (undo %s)\n") % (
                                 int(args[0]) - 1, args[1])
                except IOError:
                    desc = _("rolling back unknown transaction\n")
                self.ui.status(desc)
                if dryrun:
                    return
                transaction.rollback(self.sopener, self.sjoin("undo"),
                                     self.ui.warn)
                util.rename(self.join("undo.dirstate"), self.join("dirstate"))
                if os.path.exists(self.join('undo.bookmarks')):
                    util.rename(self.join('undo.bookmarks'),
                                self.join('bookmarks'))
                try:
                    branch = self.opener("undo.branch").read()
                    self.dirstate.setbranch(branch)
                except IOError:
                    self.ui.warn(_("Named branch could not be reset, "
                                   "current branch still is: %s\n")
                                 % self.dirstate.branch())
                self.invalidate()
                self.dirstate.invalidate()
                self.destroyed()
                parents = tuple([p.rev() for p in self.parents()])
                if len(parents) > 1:
                    self.ui.status(_("working directory now based on "
                                     "revisions %d and %d\n") % parents)
                else:
                    self.ui.status(_("working directory now based on "
                                     "revision %d\n") % parents)
            else:
                self.ui.warn(_("no rollback information available\n"))
                return 1
        finally:
            release(lock, wlock)

    def invalidatecaches(self):
        self._tags = None
        self._tagtypes = None
        self.nodetagscache = None
        self._branchcache = None # in UTF-8
        self._branchcachetip = None

    def invalidate(self):
        for a in ("changelog", "manifest", "_bookmarks", "_bookmarkcurrent"):
            if a in self.__dict__:
                delattr(self, a)
        self.invalidatecaches()

    def _lock(self, lockname, wait, releasefn, acquirefn, desc):
        try:
            l = lock.lock(lockname, 0, releasefn, desc=desc)
        except error.LockHeld, inst:
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
        '''Lock the repository store (.hg/store) and return a weak reference
        to the lock. Use this before modifying the store (e.g. committing or
        stripping). If you are opening a transaction, get a lock as well.)'''
        l = self._lockref and self._lockref()
        if l is not None and l.held:
            l.lock()
            return l

        l = self._lock(self.sjoin("lock"), wait, self.store.write,
                       self.invalidate, _('repository %s') % self.origroot)
        self._lockref = weakref.ref(l)
        return l

    def wlock(self, wait=True):
        '''Lock the non-store parts of the repository (everything under
        .hg except .hg/store) and return a weak reference to the lock.
        Use this before modifying files in .hg.'''
        l = self._wlockref and self._wlockref()
        if l is not None and l.held:
            l.lock()
            return l

        l = self._lock(self.join("wlock"), wait, self.dirstate.write,
                       self.dirstate.invalidate, _('working directory of %s') %
                       self.origroot)
        self._wlockref = weakref.ref(l)
        return l

    def _filecommit(self, fctx, manifest1, manifest2, linkrev, tr, changelist):
        """
        commit an individual file as part of a larger transaction
        """

        fname = fctx.path()
        text = fctx.data()
        flog = self.file(fname)
        fparent1 = manifest1.get(fname, nullid)
        fparent2 = fparent2o = manifest2.get(fname, nullid)

        meta = {}
        copy = fctx.renamed()
        if copy and copy[0] != fname:
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

            cfname = copy[0]
            crev = manifest1.get(cfname)
            newfparent = fparent2

            if manifest2: # branch merge
                if fparent2 == nullid or crev is None: # copied on remote side
                    if cfname in manifest2:
                        crev = manifest2[cfname]
                        newfparent = fparent1

            # find source in nearest ancestor if we've lost track
            if not crev:
                self.ui.debug(" %s: searching for copy revision for %s\n" %
                              (fname, cfname))
                for ancestor in self[None].ancestors():
                    if cfname in ancestor:
                        crev = ancestor[cfname].filenode()
                        break

            if crev:
                self.ui.debug(" %s: copy %s:%s\n" % (fname, cfname, hex(crev)))
                meta["copy"] = cfname
                meta["copyrev"] = hex(crev)
                fparent1, fparent2 = nullid, newfparent
            else:
                self.ui.warn(_("warning: can't find ancestor for '%s' "
                               "copied from '%s'!\n") % (fname, cfname))

        elif fparent2 != nullid:
            # is one parent an ancestor of the other?
            fparentancestor = flog.ancestor(fparent1, fparent2)
            if fparentancestor == fparent1:
                fparent1, fparent2 = fparent2, nullid
            elif fparentancestor == fparent2:
                fparent2 = nullid

        # is the file changed?
        if fparent2 != nullid or flog.cmp(fparent1, text) or meta:
            changelist.append(fname)
            return flog.add(text, meta, tr, linkrev, fparent1, fparent2)

        # are just the flags changed during merge?
        if fparent1 != fparent2o and manifest1.flags(fname) != fctx.flags():
            changelist.append(fname)

        return fparent1

    def commit(self, text="", user=None, date=None, match=None, force=False,
               editor=False, extra={}):
        """Add a new revision to current repository.

        Revision information is gathered from the working directory,
        match can be used to filter the committed files. If editor is
        supplied, it is called to get a commit message.
        """

        def fail(f, msg):
            raise util.Abort('%s: %s' % (f, msg))

        if not match:
            match = matchmod.always(self.root, '')

        if not force:
            vdirs = []
            match.dir = vdirs.append
            match.bad = fail

        wlock = self.wlock()
        try:
            wctx = self[None]
            merge = len(wctx.parents()) > 1

            if (not force and merge and match and
                (match.files() or match.anypats())):
                raise util.Abort(_('cannot partially commit a merge '
                                   '(do not specify files or patterns)'))

            changes = self.status(match=match, clean=force)
            if force:
                changes[0].extend(changes[6]) # mq may commit unchanged files

            # check subrepos
            subs = []
            removedsubs = set()
            for p in wctx.parents():
                removedsubs.update(s for s in p.substate if match(s))
            for s in wctx.substate:
                removedsubs.discard(s)
                if match(s) and wctx.sub(s).dirty():
                    subs.append(s)
            if (subs or removedsubs):
                if (not match('.hgsub') and
                    '.hgsub' in (wctx.modified() + wctx.added())):
                    raise util.Abort(_("can't commit subrepos without .hgsub"))
                if '.hgsubstate' not in changes[0]:
                    changes[0].insert(0, '.hgsubstate')

            if subs and not self.ui.configbool('ui', 'commitsubrepos', True):
                changedsubs = [s for s in subs if wctx.sub(s).dirty(True)]
                if changedsubs:
                    raise util.Abort(_("uncommitted changes in subrepo %s")
                                     % changedsubs[0])

            # make sure all explicit patterns are matched
            if not force and match.files():
                matched = set(changes[0] + changes[1] + changes[2])

                for f in match.files():
                    if f == '.' or f in matched or f in wctx.substate:
                        continue
                    if f in changes[3]: # missing
                        fail(f, _('file not found!'))
                    if f in vdirs: # visited directory
                        d = f + '/'
                        for mf in matched:
                            if mf.startswith(d):
                                break
                        else:
                            fail(f, _("no match under directory!"))
                    elif f not in self.dirstate:
                        fail(f, _("file not tracked!"))

            if (not force and not extra.get("close") and not merge
                and not (changes[0] or changes[1] or changes[2])
                and wctx.branch() == wctx.p1().branch()):
                return None

            ms = mergemod.mergestate(self)
            for f in changes[0]:
                if f in ms and ms[f] == 'u':
                    raise util.Abort(_("unresolved merge conflicts "
                                       "(see hg help resolve)"))

            cctx = context.workingctx(self, text, user, date, extra, changes)
            if editor:
                cctx._text = editor(self, cctx, subs)
            edited = (text != cctx._text)

            # commit subs
            if subs or removedsubs:
                state = wctx.substate.copy()
                for s in sorted(subs):
                    sub = wctx.sub(s)
                    self.ui.status(_('committing subrepository %s\n') %
                        subrepo.subrelpath(sub))
                    sr = sub.commit(cctx._text, user, date)
                    state[s] = (state[s][0], sr)
                subrepo.writestate(self, state)

            # Save commit message in case this transaction gets rolled back
            # (e.g. by a pretxncommit hook).  Leave the content alone on
            # the assumption that the user will use the same editor again.
            msgfile = self.opener('last-message.txt', 'wb')
            msgfile.write(cctx._text)
            msgfile.close()

            p1, p2 = self.dirstate.parents()
            hookp1, hookp2 = hex(p1), (p2 != nullid and hex(p2) or '')
            try:
                self.hook("precommit", throw=True, parent1=hookp1, parent2=hookp2)
                ret = self.commitctx(cctx, True)
            except:
                if edited:
                    msgfn = self.pathto(msgfile.name[len(self.root)+1:])
                    self.ui.write(
                        _('note: commit message saved in %s\n') % msgfn)
                raise

            # update bookmarks, dirstate and mergestate
            parents = (p1, p2)
            if p2 == nullid:
                parents = (p1,)
            bookmarks.update(self, parents, ret)
            for f in changes[0] + changes[1]:
                self.dirstate.normal(f)
            for f in changes[2]:
                self.dirstate.forget(f)
            self.dirstate.setparents(ret)
            ms.reset()
        finally:
            wlock.release()

        self.hook("commit", node=hex(ret), parent1=hookp1, parent2=hookp2)
        return ret

    def commitctx(self, ctx, error=False):
        """Add a new revision to current repository.
        Revision information is passed via the context argument.
        """

        tr = lock = None
        removed = list(ctx.removed())
        p1, p2 = ctx.p1(), ctx.p2()
        m1 = p1.manifest().copy()
        m2 = p2.manifest()
        user = ctx.user()

        lock = self.lock()
        try:
            tr = self.transaction("commit")
            trp = weakref.proxy(tr)

            # check in files
            new = {}
            changed = []
            linkrev = len(self)
            for f in sorted(ctx.modified() + ctx.added()):
                self.ui.note(f + "\n")
                try:
                    fctx = ctx[f]
                    new[f] = self._filecommit(fctx, m1, m2, linkrev, trp,
                                              changed)
                    m1.set(f, fctx.flags())
                except OSError, inst:
                    self.ui.warn(_("trouble committing %s!\n") % f)
                    raise
                except IOError, inst:
                    errcode = getattr(inst, 'errno', errno.ENOENT)
                    if error or errcode and errcode != errno.ENOENT:
                        self.ui.warn(_("trouble committing %s!\n") % f)
                        raise
                    else:
                        removed.append(f)

            # update manifest
            m1.update(new)
            removed = [f for f in sorted(removed) if f in m1 or f in m2]
            drop = [f for f in removed if f in m1]
            for f in drop:
                del m1[f]
            mn = self.manifest.add(m1, trp, linkrev, p1.manifestnode(),
                                   p2.manifestnode(), (new, drop))

            # update changelog
            self.changelog.delayupdate()
            n = self.changelog.add(mn, changed + removed, ctx.description(),
                                   trp, p1.node(), p2.node(),
                                   user, ctx.date(), ctx.extra().copy())
            p = lambda: self.changelog.writepending() and self.root or ""
            xp1, xp2 = p1.hex(), p2 and p2.hex() or ''
            self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                      parent2=xp2, pending=p)
            self.changelog.finalize(trp)
            tr.close()

            if self._branchcache:
                self.updatebranchcache()
            return n
        finally:
            if tr:
                tr.release()
            lock.release()

    def destroyed(self):
        '''Inform the repository that nodes have been destroyed.
        Intended for use by strip and rollback, so there's a common
        place for anything that has to be done after destroying history.'''
        # XXX it might be nice if we could take the list of destroyed
        # nodes, but I don't see an easy way for rollback() to do that

        # Ensure the persistent tag cache is updated.  Doing it now
        # means that the tag cache only has to worry about destroyed
        # heads immediately after a strip/rollback.  That in turn
        # guarantees that "cachetip == currenttip" (comparing both rev
        # and node) always means no nodes have been added or destroyed.

        # XXX this is suboptimal when qrefresh'ing: we strip the current
        # head, refresh the tag cache, then immediately add a new head.
        # But I think doing it this way is necessary for the "instant
        # tag cache retrieval" case to work.
        self.invalidatecaches()

    def walk(self, match, node=None):
        '''
        walk recursively through the directory tree or a given
        changeset, finding all files matched by the match
        function
        '''
        return self[node].walk(match)

    def status(self, node1='.', node2=None, match=None,
               ignored=False, clean=False, unknown=False,
               listsubrepos=False):
        """return status of files between two nodes or node and working directory

        If node1 is None, use the first dirstate parent instead.
        If node2 is None, compare node1 with working directory.
        """

        def mfmatches(ctx):
            mf = ctx.manifest().copy()
            for fn in mf.keys():
                if not match(fn):
                    del mf[fn]
            return mf

        if isinstance(node1, context.changectx):
            ctx1 = node1
        else:
            ctx1 = self[node1]
        if isinstance(node2, context.changectx):
            ctx2 = node2
        else:
            ctx2 = self[node2]

        working = ctx2.rev() is None
        parentworking = working and ctx1 == self['.']
        match = match or matchmod.always(self.root, self.getcwd())
        listignored, listclean, listunknown = ignored, clean, unknown

        # load earliest manifest first for caching reasons
        if not working and ctx2.rev() < ctx1.rev():
            ctx2.manifest()

        if not parentworking:
            def bad(f, msg):
                if f not in ctx1:
                    self.ui.warn('%s: %s\n' % (self.dirstate.pathto(f), msg))
            match.bad = bad

        if working: # we need to scan the working dir
            subrepos = []
            if '.hgsub' in self.dirstate:
                subrepos = ctx1.substate.keys()
            s = self.dirstate.status(match, subrepos, listignored,
                                     listclean, listunknown)
            cmp, modified, added, removed, deleted, unknown, ignored, clean = s

            # check for any possibly clean files
            if parentworking and cmp:
                fixup = []
                # do a full compare of any files that might have changed
                for f in sorted(cmp):
                    if (f not in ctx1 or ctx2.flags(f) != ctx1.flags(f)
                        or ctx1[f].cmp(ctx2[f])):
                        modified.append(f)
                    else:
                        fixup.append(f)

                # update dirstate for files that are actually clean
                if fixup:
                    if listclean:
                        clean += fixup

                    try:
                        # updating the dirstate is optional
                        # so we don't wait on the lock
                        wlock = self.wlock(False)
                        try:
                            for f in fixup:
                                self.dirstate.normal(f)
                        finally:
                            wlock.release()
                    except error.LockError:
                        pass

        if not parentworking:
            mf1 = mfmatches(ctx1)
            if working:
                # we are comparing working dir against non-parent
                # generate a pseudo-manifest for the working dir
                mf2 = mfmatches(self['.'])
                for f in cmp + modified + added:
                    mf2[f] = None
                    mf2.set(f, ctx2.flags(f))
                for f in removed:
                    if f in mf2:
                        del mf2[f]
            else:
                # we are comparing two revisions
                deleted, unknown, ignored = [], [], []
                mf2 = mfmatches(ctx2)

            modified, added, clean = [], [], []
            for fn in mf2:
                if fn in mf1:
                    if (mf1.flags(fn) != mf2.flags(fn) or
                        (mf1[fn] != mf2[fn] and
                         (mf2[fn] or ctx1[fn].cmp(ctx2[fn])))):
                        modified.append(fn)
                    elif listclean:
                        clean.append(fn)
                    del mf1[fn]
                else:
                    added.append(fn)
            removed = mf1.keys()

        r = modified, added, removed, deleted, unknown, ignored, clean

        if listsubrepos:
            for subpath, sub in subrepo.itersubrepos(ctx1, ctx2):
                if working:
                    rev2 = None
                else:
                    rev2 = ctx2.substate[subpath][1]
                try:
                    submatch = matchmod.narrowmatcher(subpath, match)
                    s = sub.status(rev2, match=submatch, ignored=listignored,
                                   clean=listclean, unknown=listunknown,
                                   listsubrepos=True)
                    for rfiles, sfiles in zip(r, s):
                        rfiles.extend("%s/%s" % (subpath, f) for f in sfiles)
                except error.LookupError:
                    self.ui.status(_("skipping missing subrepository: %s\n")
                                   % subpath)

        for l in r:
            l.sort()
        return r

    def heads(self, start=None):
        heads = self.changelog.heads(start)
        # sort the output in rev descending order
        return sorted(heads, key=self.changelog.rev, reverse=True)

    def branchheads(self, branch=None, start=None, closed=False):
        '''return a (possibly filtered) list of heads for the given branch

        Heads are returned in topological order, from newest to oldest.
        If branch is None, use the dirstate branch.
        If start is not None, return only heads reachable from start.
        If closed is True, return heads that are marked as closed as well.
        '''
        if branch is None:
            branch = self[None].branch()
        branches = self.branchmap()
        if branch not in branches:
            return []
        # the cache returns heads ordered lowest to highest
        bheads = list(reversed(branches[branch]))
        if start is not None:
            # filter out the heads that cannot be reached from startrev
            fbheads = set(self.changelog.nodesbetween([start], bheads)[2])
            bheads = [h for h in bheads if h in fbheads]
        if not closed:
            bheads = [h for h in bheads if
                      ('close' not in self.changelog.read(h)[5])]
        return bheads

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

            while n != bottom and n != nullid:
                p = self.changelog.parents(n)[0]
                if i == f:
                    l.append(n)
                    f = f * 2
                n = p
                i += 1

            r.append(l)

        return r

    def pull(self, remote, heads=None, force=False):
        lock = self.lock()
        try:
            tmp = discovery.findcommonincoming(self, remote, heads=heads,
                                               force=force)
            common, fetch, rheads = tmp
            if not fetch:
                self.ui.status(_("no changes found\n"))
                result = 0
            else:
                if heads is None and fetch == [nullid]:
                    self.ui.status(_("requesting all changes\n"))
                elif heads is None and remote.capable('changegroupsubset'):
                    # issue1320, avoid a race if remote changed after discovery
                    heads = rheads

                if heads is None:
                    cg = remote.changegroup(fetch, 'pull')
                elif not remote.capable('changegroupsubset'):
                    raise util.Abort(_("partial pull cannot be done because "
                                           "other repository doesn't support "
                                           "changegroupsubset."))
                else:
                    cg = remote.changegroupsubset(fetch, heads, 'pull')
                result = self.addchangegroup(cg, 'pull', remote.url(),
                                             lock=lock)
        finally:
            lock.release()

        self.ui.debug("checking for updated bookmarks\n")
        rb = remote.listkeys('bookmarks')
        changed = False
        for k in rb.keys():
            if k in self._bookmarks:
                nr, nl = rb[k], self._bookmarks[k]
                if nr in self:
                    cr = self[nr]
                    cl = self[nl]
                    if cl.rev() >= cr.rev():
                        continue
                    if cr in cl.descendants():
                        self._bookmarks[k] = cr.node()
                        changed = True
                        self.ui.status(_("updating bookmark %s\n") % k)
                    else:
                        self.ui.warn(_("not updating divergent"
                                       " bookmark %s\n") % k)
        if changed:
            bookmarks.write(self)

        return result

    def checkpush(self, force, revs):
        """Extensions can override this function if additional checks have
        to be performed before pushing, or call it if they override push
        command.
        """
        pass

    def push(self, remote, force=False, revs=None, newbranch=False):
        '''Push outgoing changesets (limited by revs) from the current
        repository to remote. Return an integer:
          - 0 means HTTP error *or* nothing to push
          - 1 means we pushed and remote head count is unchanged *or*
            we have outgoing changesets but refused to push
          - other values as described by addchangegroup()
        '''
        # there are two ways to push to remote repo:
        #
        # addchangegroup assumes local user can lock remote
        # repo (local filesystem, old ssh servers).
        #
        # unbundle assumes local user cannot lock remote repo (new ssh
        # servers, http servers).

        self.checkpush(force, revs)
        lock = None
        unbundle = remote.capable('unbundle')
        if not unbundle:
            lock = remote.lock()
        try:
            cg, remote_heads = discovery.prepush(self, remote, force, revs,
                                                 newbranch)
            ret = remote_heads
            if cg is not None:
                if unbundle:
                    # local repo finds heads on server, finds out what
                    # revs it must push. once revs transferred, if server
                    # finds it has different heads (someone else won
                    # commit/push race), server aborts.
                    if force:
                        remote_heads = ['force']
                    # ssh: return remote's addchangegroup()
                    # http: return remote's addchangegroup() or 0 for error
                    ret = remote.unbundle(cg, remote_heads, 'push')
                else:
                    # we return an integer indicating remote head count change
                    ret = remote.addchangegroup(cg, 'push', self.url(),
                                                lock=lock)
        finally:
            if lock is not None:
                lock.release()

        self.ui.debug("checking for updated bookmarks\n")
        rb = remote.listkeys('bookmarks')
        for k in rb.keys():
            if k in self._bookmarks:
                nr, nl = rb[k], hex(self._bookmarks[k])
                if nr in self:
                    cr = self[nr]
                    cl = self[nl]
                    if cl in cr.descendants():
                        r = remote.pushkey('bookmarks', k, nr, nl)
                        if r:
                            self.ui.status(_("updating bookmark %s\n") % k)
                        else:
                            self.ui.warn(_('updating bookmark %s'
                                           ' failed!\n') % k)

        return ret

    def changegroupinfo(self, nodes, source):
        if self.ui.verbose or source == 'bundle':
            self.ui.status(_("%d changesets found\n") % len(nodes))
        if self.ui.debugflag:
            self.ui.debug("list of changesets:\n")
            for node in nodes:
                self.ui.debug("%s\n" % hex(node))

    def changegroupsubset(self, bases, heads, source, extranodes=None):
        """Compute a changegroup consisting of all the nodes that are
        descendents of any of the bases and ancestors of any of the heads.
        Return a chunkbuffer object whose read() method will return
        successive changegroup chunks.

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

        # Set up some initial variables
        # Make it easy to refer to self.changelog
        cl = self.changelog
        # Compute the list of changesets in this changegroup.
        # Some bases may turn out to be superfluous, and some heads may be
        # too.  nodesbetween will return the minimal set of bases and heads
        # necessary to re-create the changegroup.
        if not bases:
            bases = [nullid]
        msng_cl_lst, bases, heads = cl.nodesbetween(bases, heads)

        if extranodes is None:
            # can we go through the fast path ?
            heads.sort()
            allheads = self.heads()
            allheads.sort()
            if heads == allheads:
                return self._changegroup(msng_cl_lst, source)

        # slow path
        self.hook('preoutgoing', throw=True, source=source)

        self.changegroupinfo(msng_cl_lst, source)

        # We assume that all ancestors of bases are known
        commonrevs = set(cl.ancestors(*[cl.rev(n) for n in bases]))

        # Make it easy to refer to self.manifest
        mnfst = self.manifest
        # We don't know which manifests are missing yet
        msng_mnfst_set = {}
        # Nor do we know which filenodes are missing.
        msng_filenode_set = {}

        # A changeset always belongs to itself, so the changenode lookup
        # function for a changenode is identity.
        def identity(x):
            return x

        # A function generating function that sets up the initial environment
        # the inner function.
        def filenode_collector(changedfiles):
            # This gathers information from each manifestnode included in the
            # changegroup about which filenodes the manifest node references
            # so we can include those in the changegroup too.
            #
            # It also remembers which changenode each filenode belongs to.  It
            # does this by assuming the a filenode belongs to the changenode
            # the first manifest that references it belongs to.
            def collect_msng_filenodes(mnfstnode):
                r = mnfst.rev(mnfstnode)
                if mnfst.deltaparent(r) in mnfst.parentrevs(r):
                    # If the previous rev is one of the parents,
                    # we only need to see a diff.
                    deltamf = mnfst.readdelta(mnfstnode)
                    # For each line in the delta
                    for f, fnode in deltamf.iteritems():
                        # And if the file is in the list of files we care
                        # about.
                        if f in changedfiles:
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
            return collect_msng_filenodes

        # If we determine that a particular file or manifest node must be a
        # node that the recipient of the changegroup will already have, we can
        # also assume the recipient will have all the parents.  This function
        # prunes them from the set of missing nodes.
        def prune(revlog, missingnodes):
            hasset = set()
            # If a 'missing' filenode thinks it belongs to a changenode we
            # assume the recipient must have, then the recipient must have
            # that filenode.
            for n in missingnodes:
                clrev = revlog.linkrev(revlog.rev(n))
                if clrev in commonrevs:
                    hasset.add(n)
            for n in hasset:
                missingnodes.pop(n, None)
            for r in revlog.ancestors(*[revlog.rev(n) for n in hasset]):
                missingnodes.pop(revlog.node(r), None)

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
            changedfiles = set()
            collect = changegroup.collector(cl, msng_mnfst_set, changedfiles)

            # Create a changenode group generator that will call our functions
            # back to lookup the owning changenode and collect information.
            group = cl.group(msng_cl_lst, identity, collect)
            for cnt, chnk in enumerate(group):
                yield chnk
                # revlog.group yields three entries per node, so
                # dividing by 3 gives an approximation of how many
                # nodes have been processed.
                self.ui.progress(_('bundling'), cnt / 3,
                                 unit=_('changesets'))
            changecount = cnt / 3
            self.ui.progress(_('bundling'), None)

            prune(mnfst, msng_mnfst_set)
            add_extra_nodes(1, msng_mnfst_set)
            msng_mnfst_lst = msng_mnfst_set.keys()
            # Sort the manifestnodes by revision number.
            msng_mnfst_lst.sort(key=mnfst.rev)
            # Create a generator for the manifestnodes that calls our lookup
            # and data collection functions back.
            group = mnfst.group(msng_mnfst_lst,
                                lambda mnode: msng_mnfst_set[mnode],
                                filenode_collector(changedfiles))
            efiles = {}
            for cnt, chnk in enumerate(group):
                if cnt % 3 == 1:
                    mnode = chnk[:20]
                    efiles.update(mnfst.readdelta(mnode))
                yield chnk
                # see above comment for why we divide by 3
                self.ui.progress(_('bundling'), cnt / 3,
                                 unit=_('manifests'), total=changecount)
            self.ui.progress(_('bundling'), None)
            efiles = len(efiles)

            # These are no longer needed, dereference and toss the memory for
            # them.
            msng_mnfst_lst = None
            msng_mnfst_set.clear()

            if extranodes:
                for fname in extranodes:
                    if isinstance(fname, int):
                        continue
                    msng_filenode_set.setdefault(fname, {})
                    changedfiles.add(fname)
            # Go through all our files in order sorted by name.
            for idx, fname in enumerate(sorted(changedfiles)):
                filerevlog = self.file(fname)
                if not len(filerevlog):
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                # Toss out the filenodes that the recipient isn't really
                # missing.
                missingfnodes = msng_filenode_set.pop(fname, {})
                prune(filerevlog, missingfnodes)
                add_extra_nodes(fname, missingfnodes)
                # If any filenodes are left, generate the group for them,
                # otherwise don't bother.
                if missingfnodes:
                    yield changegroup.chunkheader(len(fname))
                    yield fname
                    # Sort the filenodes by their revision # (topological order)
                    nodeiter = list(missingfnodes)
                    nodeiter.sort(key=filerevlog.rev)
                    # Create a group generator and only pass in a changenode
                    # lookup function as we need to collect no information
                    # from filenodes.
                    group = filerevlog.group(nodeiter,
                                             lambda fnode: missingfnodes[fnode])
                    for chnk in group:
                        # even though we print the same progress on
                        # most loop iterations, put the progress call
                        # here so that time estimates (if any) can be updated
                        self.ui.progress(
                            _('bundling'), idx, item=fname,
                            unit=_('files'), total=efiles)
                        yield chnk
            # Signal that no more groups are left.
            yield changegroup.closechunk()
            self.ui.progress(_('bundling'), None)

            if msng_cl_lst:
                self.hook('outgoing', node=hex(msng_cl_lst[0]), source=source)

        return changegroup.unbundle10(util.chunkbuffer(gengroup()), 'UN')

    def changegroup(self, basenodes, source):
        # to avoid a race we use changegroupsubset() (issue1320)
        return self.changegroupsubset(basenodes, self.heads(), source)

    def _changegroup(self, nodes, source):
        """Compute the changegroup of all nodes that we have that a recipient
        doesn't.  Return a chunkbuffer object whose read() method will return
        successive changegroup chunks.

        This is much easier than the previous function as we can assume that
        the recipient has any changenode we aren't sending them.

        nodes is the set of nodes to send"""

        self.hook('preoutgoing', throw=True, source=source)

        cl = self.changelog
        revset = set([cl.rev(n) for n in nodes])
        self.changegroupinfo(nodes, source)

        def identity(x):
            return x

        def gennodelst(log):
            for r in log:
                if log.linkrev(r) in revset:
                    yield log.node(r)

        def lookuplinkrev_func(revlog):
            def lookuplinkrev(n):
                return cl.node(revlog.linkrev(revlog.rev(n)))
            return lookuplinkrev

        def gengroup():
            '''yield a sequence of changegroup chunks (strings)'''
            # construct a list of all changed files
            changedfiles = set()
            mmfs = {}
            collect = changegroup.collector(cl, mmfs, changedfiles)

            for cnt, chnk in enumerate(cl.group(nodes, identity, collect)):
                # revlog.group yields three entries per node, so
                # dividing by 3 gives an approximation of how many
                # nodes have been processed.
                self.ui.progress(_('bundling'), cnt / 3, unit=_('changesets'))
                yield chnk
            changecount = cnt / 3
            self.ui.progress(_('bundling'), None)

            mnfst = self.manifest
            nodeiter = gennodelst(mnfst)
            efiles = {}
            for cnt, chnk in enumerate(mnfst.group(nodeiter,
                                                   lookuplinkrev_func(mnfst))):
                if cnt % 3 == 1:
                    mnode = chnk[:20]
                    efiles.update(mnfst.readdelta(mnode))
                # see above comment for why we divide by 3
                self.ui.progress(_('bundling'), cnt / 3,
                                 unit=_('manifests'), total=changecount)
                yield chnk
            efiles = len(efiles)
            self.ui.progress(_('bundling'), None)

            for idx, fname in enumerate(sorted(changedfiles)):
                filerevlog = self.file(fname)
                if not len(filerevlog):
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                nodeiter = gennodelst(filerevlog)
                nodeiter = list(nodeiter)
                if nodeiter:
                    yield changegroup.chunkheader(len(fname))
                    yield fname
                    lookup = lookuplinkrev_func(filerevlog)
                    for chnk in filerevlog.group(nodeiter, lookup):
                        self.ui.progress(
                            _('bundling'), idx, item=fname,
                            total=efiles, unit=_('files'))
                        yield chnk
            self.ui.progress(_('bundling'), None)

            yield changegroup.closechunk()

            if nodes:
                self.hook('outgoing', node=hex(nodes[0]), source=source)

        return changegroup.unbundle10(util.chunkbuffer(gengroup()), 'UN')

    def addchangegroup(self, source, srctype, url, emptyok=False, lock=None):
        """Add the changegroup returned by source.read() to this repo.
        srctype is a string like 'push', 'pull', or 'unbundle'.  url is
        the URL of the repo where this changegroup is coming from.
        If lock is not None, the function takes ownership of the lock
        and releases it after the changegroup is added.

        Return an integer summarizing the change to this repo:
        - nothing changed or no source: 0
        - more heads than before: 1+added heads (2..n)
        - fewer heads than before: -1-removed heads (-2..-n)
        - number of heads stays the same: 1
        """
        def csmap(x):
            self.ui.debug("add changeset %s\n" % short(x))
            return len(cl)

        def revmap(x):
            return cl.rev(x)

        if not source:
            return 0

        self.hook('prechangegroup', throw=True, source=srctype, url=url)

        changesets = files = revisions = 0
        efiles = set()

        # write changelog data to temp files so concurrent readers will not see
        # inconsistent view
        cl = self.changelog
        cl.delayupdate()
        oldheads = len(cl.heads())

        tr = self.transaction("\n".join([srctype, urlmod.hidepassword(url)]))
        try:
            trp = weakref.proxy(tr)
            # pull off the changeset group
            self.ui.status(_("adding changesets\n"))
            clstart = len(cl)
            class prog(object):
                step = _('changesets')
                count = 1
                ui = self.ui
                total = None
                def __call__(self):
                    self.ui.progress(self.step, self.count, unit=_('chunks'),
                                     total=self.total)
                    self.count += 1
            pr = prog()
            source.callback = pr

            if (cl.addgroup(source, csmap, trp) is None
                and not emptyok):
                raise util.Abort(_("received changelog group is empty"))
            clend = len(cl)
            changesets = clend - clstart
            for c in xrange(clstart, clend):
                efiles.update(self[c].files())
            efiles = len(efiles)
            self.ui.progress(_('changesets'), None)

            # pull off the manifest group
            self.ui.status(_("adding manifests\n"))
            pr.step = _('manifests')
            pr.count = 1
            pr.total = changesets # manifests <= changesets
            # no need to check for empty manifest group here:
            # if the result of the merge of 1 and 2 is the same in 3 and 4,
            # no new manifest will be created and the manifest group will
            # be empty during the pull
            self.manifest.addgroup(source, revmap, trp)
            self.ui.progress(_('manifests'), None)

            needfiles = {}
            if self.ui.configbool('server', 'validate', default=False):
                # validate incoming csets have their manifests
                for cset in xrange(clstart, clend):
                    mfest = self.changelog.read(self.changelog.node(cset))[0]
                    mfest = self.manifest.readdelta(mfest)
                    # store file nodes we must see
                    for f, n in mfest.iteritems():
                        needfiles.setdefault(f, set()).add(n)

            # process the files
            self.ui.status(_("adding file changes\n"))
            pr.step = 'files'
            pr.count = 1
            pr.total = efiles
            source.callback = None

            while 1:
                f = source.chunk()
                if not f:
                    break
                self.ui.debug("adding %s revisions\n" % f)
                pr()
                fl = self.file(f)
                o = len(fl)
                if fl.addgroup(source, revmap, trp) is None:
                    raise util.Abort(_("received file revlog group is empty"))
                revisions += len(fl) - o
                files += 1
                if f in needfiles:
                    needs = needfiles[f]
                    for new in xrange(o, len(fl)):
                        n = fl.node(new)
                        if n in needs:
                            needs.remove(n)
                    if not needs:
                        del needfiles[f]
            self.ui.progress(_('files'), None)

            for f, needs in needfiles.iteritems():
                fl = self.file(f)
                for n in needs:
                    try:
                        fl.rev(n)
                    except error.LookupError:
                        raise util.Abort(
                            _('missing file data for %s:%s - run hg verify') %
                            (f, hex(n)))

            newheads = len(cl.heads())
            heads = ""
            if oldheads and newheads != oldheads:
                heads = _(" (%+d heads)") % (newheads - oldheads)

            self.ui.status(_("added %d changesets"
                             " with %d changes to %d files%s\n")
                             % (changesets, revisions, files, heads))

            if changesets > 0:
                p = lambda: cl.writepending() and self.root or ""
                self.hook('pretxnchangegroup', throw=True,
                          node=hex(cl.node(clstart)), source=srctype,
                          url=url, pending=p)

            # make changelog see real files again
            cl.finalize(trp)

            tr.close()
        finally:
            tr.release()
            if lock:
                lock.release()

        if changesets > 0:
            # forcefully update the on-disk branch cache
            self.ui.debug("updating the branch cache\n")
            self.updatebranchcache()
            self.hook("changegroup", node=hex(cl.node(clstart)),
                      source=srctype, url=url)

            for i in xrange(clstart, clend):
                self.hook("incoming", node=hex(cl.node(i)),
                          source=srctype, url=url)

        # FIXME - why does this care about tip?
        if newheads == oldheads:
            bookmarks.update(self, self.dirstate.parents(), self['tip'].node())

        # never return 0 here:
        if newheads < oldheads:
            return newheads - oldheads - 1
        else:
            return newheads - oldheads + 1


    def stream_in(self, remote, requirements):
        lock = self.lock()
        try:
            fp = remote.stream_out()
            l = fp.readline()
            try:
                resp = int(l)
            except ValueError:
                raise error.ResponseError(
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
                raise error.ResponseError(
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
                except (ValueError, TypeError):
                    raise error.ResponseError(
                        _('Unexpected response from remote server:'), l)
                self.ui.debug('adding %s (%s)\n' % (name, util.bytecount(size)))
                # for backwards compat, name was partially encoded
                ofp = self.sopener(store.decodedir(name), 'w')
                for chunk in util.filechunkiter(fp, limit=size):
                    ofp.write(chunk)
                ofp.close()
            elapsed = time.time() - start
            if elapsed <= 0:
                elapsed = 0.001
            self.ui.status(_('transferred %s in %.1f seconds (%s/sec)\n') %
                           (util.bytecount(total_bytes), elapsed,
                            util.bytecount(total_bytes / elapsed)))

            # new requirements = old non-format requirements + new format-related
            # requirements from the streamed-in repository
            requirements.update(set(self.requirements) - self.supportedformats)
            self._applyrequirements(requirements)
            self._writerequirements()

            self.invalidate()
            return len(self.heads()) + 1
        finally:
            lock.release()

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

        if stream and not heads:
            # 'stream' means remote revlog format is revlogv1 only
            if remote.capable('stream'):
                return self.stream_in(remote, set(('revlogv1',)))
            # otherwise, 'streamreqs' contains the remote revlog format
            streamreqs = remote.capable('streamreqs')
            if streamreqs:
                streamreqs = set(streamreqs.split(','))
                # if we support it, stream in and adjust our requirements
                if not streamreqs - self.supportedformats:
                    return self.stream_in(remote, streamreqs)
        return self.pull(remote, heads)

    def pushkey(self, namespace, key, old, new):
        return pushkey.push(self, namespace, key, old, new)

    def listkeys(self, namespace):
        return pushkey.list(self, namespace)

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
