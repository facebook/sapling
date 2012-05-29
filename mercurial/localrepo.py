# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev, short
from i18n import _
import repo, changegroup, subrepo, discovery, pushkey
import changelog, dirstate, filelog, manifest, context, bookmarks, phases
import lock, transaction, store, encoding
import scmutil, util, extensions, hook, error, revset
import match as matchmod
import merge as mergemod
import tags as tagsmod
from lock import release
import weakref, errno, os, time, inspect
propertycache = util.propertycache
filecache = scmutil.filecache

class storecache(filecache):
    """filecache for files in the store"""
    def join(self, obj, fname):
        return obj.sjoin(fname)

class localrepository(repo.repository):
    capabilities = set(('lookup', 'changegroupsubset', 'branchmap', 'pushkey',
                        'known', 'getbundle'))
    supportedformats = set(('revlogv1', 'generaldelta'))
    supported = supportedformats | set(('store', 'fncache', 'shared',
                                        'dotencode'))

    def __init__(self, baseui, path=None, create=False):
        repo.repository.__init__(self)
        self.root = os.path.realpath(util.expandpath(path))
        self.path = os.path.join(self.root, ".hg")
        self.origroot = path
        self.auditor = scmutil.pathauditor(self.root, self._checknested)
        self.opener = scmutil.opener(self.path)
        self.wopener = scmutil.opener(self.root)
        self.baseui = baseui
        self.ui = baseui.copy()
        self._dirtyphases = False
        # A list of callback to shape the phase if no data were found.
        # Callback are in the form: func(repo, roots) --> processed root.
        # This list it to be filled by extension during repo setup
        self._phasedefaults = []

        try:
            self.ui.readconfig(self.join("hgrc"), self.root)
            extensions.loadall(self.ui)
        except IOError:
            pass

        if not os.path.isdir(self.path):
            if create:
                if not os.path.exists(path):
                    util.makedirs(path)
                util.makedir(self.path, notindexed=True)
                requirements = ["revlogv1"]
                if self.ui.configbool('format', 'usestore', True):
                    os.mkdir(os.path.join(self.path, "store"))
                    requirements.append("store")
                    if self.ui.configbool('format', 'usefncache', True):
                        requirements.append("fncache")
                        if self.ui.configbool('format', 'dotencode', True):
                            requirements.append('dotencode')
                    # create an invalid changelog
                    self.opener.append(
                        "00changelog.i",
                        '\0\0\0\2' # represents revlogv2
                        ' dummy changelog to prevent using the old repo layout'
                    )
                if self.ui.configbool('format', 'generaldelta', False):
                    requirements.append("generaldelta")
                requirements = set(requirements)
            else:
                raise error.RepoError(_("repository %s not found") % path)
        elif create:
            raise error.RepoError(_("repository %s already exists") % path)
        else:
            try:
                requirements = scmutil.readrequires(self.opener, self.supported)
            except IOError, inst:
                if inst.errno != errno.ENOENT:
                    raise
                requirements = set()

        self.sharedpath = self.path
        try:
            s = os.path.realpath(self.opener.read("sharedpath").rstrip('\n'))
            if not os.path.exists(s):
                raise error.RepoError(
                    _('.hg/sharedpath points to nonexistent directory %s') % s)
            self.sharedpath = s
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise

        self.store = store.store(requirements, self.sharedpath, scmutil.opener)
        self.spath = self.store.path
        self.sopener = self.store.opener
        self.sjoin = self.store.join
        self.opener.createmode = self.store.createmode
        self._applyrequirements(requirements)
        if create:
            self._writerequirements()


        self._branchcache = None
        self._branchcachetip = None
        self.filterpats = {}
        self._datafilters = {}
        self._transref = self._lockref = self._wlockref = None

        # A cache for various files under .hg/ that tracks file changes,
        # (used by the filecache decorator)
        #
        # Maps a property name to its util.filecacheentry
        self._filecache = {}

    def _applyrequirements(self, requirements):
        self.requirements = requirements
        openerreqs = set(('revlogv1', 'generaldelta'))
        self.sopener.options = dict((r, 1) for r in requirements
                                           if r in openerreqs)

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
        normsubpath = util.pconvert(subpath)

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
            prefix = '/'.join(parts)
            if prefix in ctx.substate:
                if prefix == normsubpath:
                    return True
                else:
                    sub = ctx.sub(prefix)
                    return sub.checknested(subpath[len(prefix) + 1:])
            else:
                parts.pop()
        return False

    @filecache('bookmarks')
    def _bookmarks(self):
        return bookmarks.read(self)

    @filecache('bookmarks.current')
    def _bookmarkcurrent(self):
        return bookmarks.readcurrent(self)

    def _writebookmarks(self, marks):
      bookmarks.write(self)

    @storecache('phaseroots')
    def _phaseroots(self):
        self._dirtyphases = False
        phaseroots = phases.readroots(self)
        phases.filterunknown(self, phaseroots)
        return phaseroots

    @propertycache
    def _phaserev(self):
        cache = [phases.public] * len(self)
        for phase in phases.trackedphases:
            roots = map(self.changelog.rev, self._phaseroots[phase])
            if roots:
                for rev in roots:
                    cache[rev] = phase
                for rev in self.changelog.descendants(*roots):
                    cache[rev] = phase
        return cache

    @storecache('00changelog.i')
    def changelog(self):
        c = changelog.changelog(self.sopener)
        if 'HG_PENDING' in os.environ:
            p = os.environ['HG_PENDING']
            if p.startswith(self.root):
                c.readpending('00changelog.i.a')
        return c

    @storecache('00manifest.i')
    def manifest(self):
        return manifest.manifest(self.sopener)

    @filecache('dirstate')
    def dirstate(self):
        warned = [0]
        def validate(node):
            try:
                self.changelog.rev(node)
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

    def revs(self, expr, *args):
        '''Return a list of revisions matching the given revset'''
        expr = revset.formatspec(expr, *args)
        m = revset.match(None, expr)
        return [r for r in m(self, range(len(self)))]

    def set(self, expr, *args):
        '''
        Yield a context for each matching revision, after doing arg
        replacement via revset.formatspec
        '''
        for r in self.revs(expr, *args):
            yield self[r]

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
                if self._tagscache.tagtypes and name in self._tagscache.tagtypes:
                    old = self.tags().get(name, nullid)
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
        except IOError, e:
            if e.errno != errno.ENOENT:
                raise
            fp = self.wfile('.hgtags', 'ab')
        else:
            prevtags = fp.read()

        # committed tags are stored in UTF-8
        writetags(fp, names, encoding.fromlocal, prevtags)

        fp.close()

        self.invalidatecaches()

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

    @propertycache
    def _tagscache(self):
        '''Returns a tagscache object that contains various tags related caches.'''

        # This simplifies its cache management by having one decorated
        # function (this one) and the rest simply fetch things from it.
        class tagscache(object):
            def __init__(self):
                # These two define the set of tags for this repository. tags
                # maps tag name to node; tagtypes maps tag name to 'global' or
                # 'local'. (Global tags are defined by .hgtags across all
                # heads, and local tags are defined in .hg/localtags.)
                # They constitute the in-memory cache of tags.
                self.tags = self.tagtypes = None

                self.nodetagscache = self.tagslist = None

        cache = tagscache()
        cache.tags, cache.tagtypes = self._findtags()

        return cache

    def tags(self):
        '''return a mapping of tag to node'''
        t = {}
        for k, v in self._tagscache.tags.iteritems():
            try:
                # ignore tags to unknown nodes
                self.changelog.rev(v)
                t[k] = v
            except (error.LookupError, ValueError):
                pass
        return t

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

        return self._tagscache.tagtypes.get(tagname)

    def tagslist(self):
        '''return a list of tags ordered by revision'''
        if not self._tagscache.tagslist:
            l = []
            for t, n in self.tags().iteritems():
                r = self.changelog.rev(n)
                l.append((r, t, n))
            self._tagscache.tagslist = [(t, n) for r, t, n in sorted(l)]

        return self._tagscache.tagslist

    def nodetags(self, node):
        '''return the tags associated with a node'''
        if not self._tagscache.nodetagscache:
            nodetagscache = {}
            for t, n in self._tagscache.tags.iteritems():
                nodetagscache.setdefault(n, []).append(t)
            for tags in nodetagscache.itervalues():
                tags.sort()
            self._tagscache.nodetagscache = nodetagscache
        return self._tagscache.nodetagscache.get(node, [])

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
            return

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
            f.close()
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
            bheads = sorted(bheads, key=lambda x: self[x].rev())
            # starting from tip means fewer passes over reachable
            while newnodes:
                latest = newnodes.pop()
                if latest not in bheads:
                    continue
                minbhrev = self[bheads[0]].node()
                reachable = self.changelog.reachable(latest, minbhrev)
                reachable.remove(latest)
                if reachable:
                    bheads = [b for b in bheads if b not in reachable]
            partial[branch] = bheads

    def lookup(self, key):
        return self[key].node()

    def lookupbranch(self, key, remote=None):
        repo = remote or self
        if key in repo.branchmap():
            return key

        repo = (remote and remote.local()) and remote or self
        return repo[key].branch()

    def known(self, nodes):
        nm = self.changelog.nodemap
        result = []
        for n in nodes:
            r = nm.get(n)
            resp = not (r is None or self._phaserev[r] >= phases.secret)
            result.append(resp)
        return result

    def local(self):
        return self

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

    def setparents(self, p1, p2=nullid):
        copies = self.dirstate.setparents(p1, p2)
        if copies:
            # Adjust copy records, the dirstate cannot do it, it
            # requires access to parents manifests. Preserve them
            # only for entries added to first parent.
            pctx = self[p1]
            for f in copies:
                if f not in pctx and copies[f] in pctx:
                    self.dirstate.copy(copies[f], f)

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
            data = self.wopener.read(filename)
        return self._filter(self._encodefilterpats, filename, data)

    def wwrite(self, filename, data, flags):
        data = self._filter(self._decodefilterpats, filename, data)
        if 'l' in flags:
            self.wopener.symlink(data, filename)
        else:
            self.wopener.write(filename, data)
            if 'x' in flags:
                util.setflags(self.wjoin(filename), False, True)

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

        self._writejournal(desc)
        renames = [(x, undoname(x)) for x in self._journalfiles()]

        tr = transaction.transaction(self.ui.warn, self.sopener,
                                     self.sjoin("journal"),
                                     aftertrans(renames),
                                     self.store.createmode)
        self._transref = weakref.ref(tr)
        return tr

    def _journalfiles(self):
        return (self.sjoin('journal'), self.join('journal.dirstate'),
                self.join('journal.branch'), self.join('journal.desc'),
                self.join('journal.bookmarks'),
                self.sjoin('journal.phaseroots'))

    def undofiles(self):
        return [undoname(x) for x in self._journalfiles()]

    def _writejournal(self, desc):
        self.opener.write("journal.dirstate",
                          self.opener.tryread("dirstate"))
        self.opener.write("journal.branch",
                          encoding.fromlocal(self.dirstate.branch()))
        self.opener.write("journal.desc",
                          "%d\n%s\n" % (len(self), desc))
        self.opener.write("journal.bookmarks",
                          self.opener.tryread("bookmarks"))
        self.sopener.write("journal.phaseroots",
                           self.sopener.tryread("phaseroots"))

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

    def rollback(self, dryrun=False, force=False):
        wlock = lock = None
        try:
            wlock = self.wlock()
            lock = self.lock()
            if os.path.exists(self.sjoin("undo")):
                return self._rollback(dryrun, force)
            else:
                self.ui.warn(_("no rollback information available\n"))
                return 1
        finally:
            release(lock, wlock)

    def _rollback(self, dryrun, force):
        ui = self.ui
        try:
            args = self.opener.read('undo.desc').splitlines()
            (oldlen, desc, detail) = (int(args[0]), args[1], None)
            if len(args) >= 3:
                detail = args[2]
            oldtip = oldlen - 1

            if detail and ui.verbose:
                msg = (_('repository tip rolled back to revision %s'
                         ' (undo %s: %s)\n')
                       % (oldtip, desc, detail))
            else:
                msg = (_('repository tip rolled back to revision %s'
                         ' (undo %s)\n')
                       % (oldtip, desc))
        except IOError:
            msg = _('rolling back unknown transaction\n')
            desc = None

        if not force and self['.'] != self['tip'] and desc == 'commit':
            raise util.Abort(
                _('rollback of last commit while not checked out '
                  'may lose data'), hint=_('use -f to force'))

        ui.status(msg)
        if dryrun:
            return 0

        parents = self.dirstate.parents()
        transaction.rollback(self.sopener, self.sjoin('undo'), ui.warn)
        if os.path.exists(self.join('undo.bookmarks')):
            util.rename(self.join('undo.bookmarks'),
                        self.join('bookmarks'))
        if os.path.exists(self.sjoin('undo.phaseroots')):
            util.rename(self.sjoin('undo.phaseroots'),
                        self.sjoin('phaseroots'))
        self.invalidate()

        # Discard all cache entries to force reloading everything.
        self._filecache.clear()

        parentgone = (parents[0] not in self.changelog.nodemap or
                      parents[1] not in self.changelog.nodemap)
        if parentgone:
            util.rename(self.join('undo.dirstate'), self.join('dirstate'))
            try:
                branch = self.opener.read('undo.branch')
                self.dirstate.setbranch(branch)
            except IOError:
                ui.warn(_('named branch could not be reset: '
                          'current branch is still \'%s\'\n')
                        % self.dirstate.branch())

            self.dirstate.invalidate()
            parents = tuple([p.rev() for p in self.parents()])
            if len(parents) > 1:
                ui.status(_('working directory now based on '
                            'revisions %d and %d\n') % parents)
            else:
                ui.status(_('working directory now based on '
                            'revision %d\n') % parents)
        self.destroyed()
        return 0

    def invalidatecaches(self):
        def delcache(name):
            try:
                delattr(self, name)
            except AttributeError:
                pass

        delcache('_tagscache')
        delcache('_phaserev')

        self._branchcache = None # in UTF-8
        self._branchcachetip = None

    def invalidatedirstate(self):
        '''Invalidates the dirstate, causing the next call to dirstate
        to check if it was modified since the last time it was read,
        rereading it if it has.

        This is different to dirstate.invalidate() that it doesn't always
        rereads the dirstate. Use dirstate.invalidate() if you want to
        explicitly read the dirstate again (i.e. restoring it to a previous
        known good state).'''
        if 'dirstate' in self.__dict__:
            for k in self.dirstate._filecache:
                try:
                    delattr(self.dirstate, k)
                except AttributeError:
                    pass
            delattr(self, 'dirstate')

    def invalidate(self):
        for k in self._filecache:
            # dirstate is invalidated separately in invalidatedirstate()
            if k == 'dirstate':
                continue

            try:
                delattr(self, k)
            except AttributeError:
                pass
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

    def _afterlock(self, callback):
        """add a callback to the current repository lock.

        The callback will be executed on lock release."""
        l = self._lockref and self._lockref()
        if l:
            l.postrelease.append(callback)
        else:
            callback()

    def lock(self, wait=True):
        '''Lock the repository store (.hg/store) and return a weak reference
        to the lock. Use this before modifying the store (e.g. committing or
        stripping). If you are opening a transaction, get a lock as well.)'''
        l = self._lockref and self._lockref()
        if l is not None and l.held:
            l.lock()
            return l

        def unlock():
            self.store.write()
            if self._dirtyphases:
                phases.writeroots(self)
                self._dirtyphases = False
            for k, ce in self._filecache.items():
                if k == 'dirstate':
                    continue
                ce.refresh()

        l = self._lock(self.sjoin("lock"), wait, unlock,
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

        def unlock():
            self.dirstate.write()
            ce = self._filecache.get('dirstate')
            if ce:
                ce.refresh()

        l = self._lock(self.join("wlock"), wait, unlock,
                       self.invalidatedirstate, _('working directory of %s') %
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
            commitsubs = set()
            newstate = wctx.substate.copy()
            # only manage subrepos and .hgsubstate if .hgsub is present
            if '.hgsub' in wctx:
                # we'll decide whether to track this ourselves, thanks
                if '.hgsubstate' in changes[0]:
                    changes[0].remove('.hgsubstate')
                if '.hgsubstate' in changes[2]:
                    changes[2].remove('.hgsubstate')

                # compare current state to last committed state
                # build new substate based on last committed state
                oldstate = wctx.p1().substate
                for s in sorted(newstate.keys()):
                    if not match(s):
                        # ignore working copy, use old state if present
                        if s in oldstate:
                            newstate[s] = oldstate[s]
                            continue
                        if not force:
                            raise util.Abort(
                                _("commit with new subrepo %s excluded") % s)
                    if wctx.sub(s).dirty(True):
                        if not self.ui.configbool('ui', 'commitsubrepos'):
                            raise util.Abort(
                                _("uncommitted changes in subrepo %s") % s,
                                hint=_("use --subrepos for recursive commit"))
                        subs.append(s)
                        commitsubs.add(s)
                    else:
                        bs = wctx.sub(s).basestate()
                        newstate[s] = (newstate[s][0], bs, newstate[s][2])
                        if oldstate.get(s, (None, None, None))[1] != bs:
                            subs.append(s)

                # check for removed subrepos
                for p in wctx.parents():
                    r = [s for s in p.substate if s not in newstate]
                    subs += [s for s in r if match(s)]
                if subs:
                    if (not match('.hgsub') and
                        '.hgsub' in (wctx.modified() + wctx.added())):
                        raise util.Abort(
                            _("can't commit subrepos without .hgsub"))
                    changes[0].insert(0, '.hgsubstate')

            elif '.hgsub' in changes[2]:
                # clean up .hgsubstate when .hgsub is removed
                if ('.hgsubstate' in wctx and
                    '.hgsubstate' not in changes[0] + changes[1] + changes[2]):
                    changes[2].insert(0, '.hgsubstate')

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

            if merge and changes[3]:
                raise util.Abort(_("cannot commit merge with missing files"))

            ms = mergemod.mergestate(self)
            for f in changes[0]:
                if f in ms and ms[f] == 'u':
                    raise util.Abort(_("unresolved merge conflicts "
                                       "(see hg help resolve)"))

            cctx = context.workingctx(self, text, user, date, extra, changes)
            if editor:
                cctx._text = editor(self, cctx, subs)
            edited = (text != cctx._text)

            # commit subs and write new state
            if subs:
                for s in sorted(commitsubs):
                    sub = wctx.sub(s)
                    self.ui.status(_('committing subrepository %s\n') %
                        subrepo.subrelpath(sub))
                    sr = sub.commit(cctx._text, user, date)
                    newstate[s] = (newstate[s][0], sr)
                subrepo.writestate(self, newstate)

            # Save commit message in case this transaction gets rolled back
            # (e.g. by a pretxncommit hook).  Leave the content alone on
            # the assumption that the user will use the same editor again.
            msgfn = self.savecommitmessage(cctx._text)

            p1, p2 = self.dirstate.parents()
            hookp1, hookp2 = hex(p1), (p2 != nullid and hex(p2) or '')
            try:
                self.hook("precommit", throw=True, parent1=hookp1, parent2=hookp2)
                ret = self.commitctx(cctx, True)
            except:
                if edited:
                    self.ui.write(
                        _('note: commit message saved in %s\n') % msgfn)
                raise

            # update bookmarks, dirstate and mergestate
            bookmarks.update(self, p1, ret)
            for f in changes[0] + changes[1]:
                self.dirstate.normal(f)
            for f in changes[2]:
                self.dirstate.drop(f)
            self.dirstate.setparents(ret)
            ms.reset()
        finally:
            wlock.release()

        def commithook(node=hex(ret), parent1=hookp1, parent2=hookp2):
            self.hook("commit", node=node, parent1=parent1, parent2=parent2)
        self._afterlock(commithook)
        return ret

    def commitctx(self, ctx, error=False):
        """Add a new revision to current repository.
        Revision information is passed via the context argument.
        """

        tr = lock = None
        removed = list(ctx.removed())
        p1, p2 = ctx.p1(), ctx.p2()
        user = ctx.user()

        lock = self.lock()
        try:
            tr = self.transaction("commit")
            trp = weakref.proxy(tr)

            if ctx.files():
                m1 = p1.manifest().copy()
                m2 = p2.manifest()

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
                files = changed + removed
            else:
                mn = p1.manifestnode()
                files = []

            # update changelog
            self.changelog.delayupdate()
            n = self.changelog.add(mn, files, ctx.description(),
                                   trp, p1.node(), p2.node(),
                                   user, ctx.date(), ctx.extra().copy())
            p = lambda: self.changelog.writepending() and self.root or ""
            xp1, xp2 = p1.hex(), p2 and p2.hex() or ''
            self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                      parent2=xp2, pending=p)
            self.changelog.finalize(trp)
            # set the new commit is proper phase
            targetphase = phases.newcommitphase(self.ui)
            if targetphase:
                # retract boundary do not alter parent changeset.
                # if a parent have higher the resulting phase will
                # be compliant anyway
                #
                # if minimal phase was 0 we don't need to retract anything
                phases.retractboundary(self, targetphase, [n])
            tr.close()
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
                # 'f' may be a directory pattern from 'match.files()',
                # so 'f not in ctx1' is not enough
                if f not in ctx1 and f not in ctx1.dirs():
                    self.ui.warn('%s: %s\n' % (self.dirstate.pathto(f), msg))
            match.bad = bad

        if working: # we need to scan the working dir
            subrepos = []
            if '.hgsub' in self.dirstate:
                subrepos = ctx2.substate.keys()
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
                    if (fn not in deleted and
                        (mf1.flags(fn) != mf2.flags(fn) or
                         (mf1[fn] != mf2[fn] and
                          (mf2[fn] or ctx1[fn].cmp(ctx2[fn]))))):
                        modified.append(fn)
                    elif listclean:
                        clean.append(fn)
                    del mf1[fn]
                elif fn not in deleted:
                    added.append(fn)
            removed = mf1.keys()

        if working and modified and not self.dirstate._checklink:
            # Symlink placeholders may get non-symlink-like contents
            # via user error or dereferencing by NFS or Samba servers,
            # so we filter out any placeholders that don't look like a
            # symlink
            sane = []
            for f in modified:
                if ctx2.flags(f) == 'l':
                    d = ctx2[f].data()
                    if len(d) >= 1024 or '\n' in d or util.binary(d):
                        self.ui.debug('ignoring suspect symlink placeholder'
                                      ' "%s"\n' % f)
                        continue
                sane.append(f)
            modified = sane

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
            while True:
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
                added = []
                result = 0
            else:
                if heads is None and list(common) == [nullid]:
                    self.ui.status(_("requesting all changes\n"))
                elif heads is None and remote.capable('changegroupsubset'):
                    # issue1320, avoid a race if remote changed after discovery
                    heads = rheads

                if remote.capable('getbundle'):
                    cg = remote.getbundle('pull', common=common,
                                          heads=heads or rheads)
                elif heads is None:
                    cg = remote.changegroup(fetch, 'pull')
                elif not remote.capable('changegroupsubset'):
                    raise util.Abort(_("partial pull cannot be done because "
                                           "other repository doesn't support "
                                           "changegroupsubset."))
                else:
                    cg = remote.changegroupsubset(fetch, heads, 'pull')
                clstart = len(self.changelog)
                result = self.addchangegroup(cg, 'pull', remote.url())
                clend = len(self.changelog)
                added = [self.changelog.node(r) for r in xrange(clstart, clend)]

            # compute target subset
            if heads is None:
                # We pulled every thing possible
                # sync on everything common
                subset = common + added
            else:
                # We pulled a specific subset
                # sync on this subset
                subset = heads

            # Get remote phases data from remote
            remotephases = remote.listkeys('phases')
            publishing = bool(remotephases.get('publishing', False))
            if remotephases and not publishing:
                # remote is new and unpublishing
                pheads, _dr = phases.analyzeremotephases(self, subset,
                                                         remotephases)
                phases.advanceboundary(self, phases.public, pheads)
                phases.advanceboundary(self, phases.draft, subset)
            else:
                # Remote is old or publishing all common changesets
                # should be seen as public
                phases.advanceboundary(self, phases.public, subset)
        finally:
            lock.release()

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
          - None means nothing to push
          - 0 means HTTP error
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

        # get local lock as we might write phase data
        locallock = self.lock()
        try:
            self.checkpush(force, revs)
            lock = None
            unbundle = remote.capable('unbundle')
            if not unbundle:
                lock = remote.lock()
            try:
                # discovery
                fci = discovery.findcommonincoming
                commoninc = fci(self, remote, force=force)
                common, inc, remoteheads = commoninc
                fco = discovery.findcommonoutgoing
                outgoing = fco(self, remote, onlyheads=revs,
                               commoninc=commoninc, force=force)


                if not outgoing.missing:
                    # nothing to push
                    scmutil.nochangesfound(self.ui, outgoing.excluded)
                    ret = None
                else:
                    # something to push
                    if not force:
                        discovery.checkheads(self, remote, outgoing,
                                             remoteheads, newbranch,
                                             bool(inc))

                    # create a changegroup from local
                    if revs is None and not outgoing.excluded:
                        # push everything,
                        # use the fast path, no race possible on push
                        cg = self._changegroup(outgoing.missing, 'push')
                    else:
                        cg = self.getlocalbundle('push', outgoing)

                    # apply changegroup to remote
                    if unbundle:
                        # local repo finds heads on server, finds out what
                        # revs it must push. once revs transferred, if server
                        # finds it has different heads (someone else won
                        # commit/push race), server aborts.
                        if force:
                            remoteheads = ['force']
                        # ssh: return remote's addchangegroup()
                        # http: return remote's addchangegroup() or 0 for error
                        ret = remote.unbundle(cg, remoteheads, 'push')
                    else:
                        # we return an integer indicating remote head count change
                        ret = remote.addchangegroup(cg, 'push', self.url())

                if ret:
                    # push succeed, synchonize target of the push
                    cheads = outgoing.missingheads
                elif revs is None:
                    # All out push fails. synchronize all common
                    cheads = outgoing.commonheads
                else:
                    # I want cheads = heads(::missingheads and ::commonheads)
                    # (missingheads is revs with secret changeset filtered out)
                    #
                    # This can be expressed as:
                    #     cheads = ( (missingheads and ::commonheads)
                    #              + (commonheads and ::missingheads))"
                    #              )
                    #
                    # while trying to push we already computed the following:
                    #     common = (::commonheads)
                    #     missing = ((commonheads::missingheads) - commonheads)
                    #
                    # We can pick:
                    # * missingheads part of comon (::commonheads)
                    common = set(outgoing.common)
                    cheads = [node for node in revs if node in common]
                    # and 
                    # * commonheads parents on missing
                    revset = self.set('%ln and parents(roots(%ln))',
                                     outgoing.commonheads,
                                     outgoing.missing)
                    cheads.extend(c.node() for c in revset)
                # even when we don't push, exchanging phase data is useful
                remotephases = remote.listkeys('phases')
                if not remotephases: # old server or public only repo
                    phases.advanceboundary(self, phases.public, cheads)
                    # don't push any phase data as there is nothing to push
                else:
                    ana = phases.analyzeremotephases(self, cheads, remotephases)
                    pheads, droots = ana
                    ### Apply remote phase on local
                    if remotephases.get('publishing', False):
                        phases.advanceboundary(self, phases.public, cheads)
                    else: # publish = False
                        phases.advanceboundary(self, phases.public, pheads)
                        phases.advanceboundary(self, phases.draft, cheads)
                    ### Apply local phase on remote

                    # Get the list of all revs draft on remote by public here.
                    # XXX Beware that revset break if droots is not strictly
                    # XXX root we may want to ensure it is but it is costly
                    outdated =  self.set('heads((%ln::%ln) and public())',
                                         droots, cheads)
                    for newremotehead in outdated:
                        r = remote.pushkey('phases',
                                           newremotehead.hex(),
                                           str(phases.draft),
                                           str(phases.public))
                        if not r:
                            self.ui.warn(_('updating %s to public failed!\n')
                                            % newremotehead)
            finally:
                if lock is not None:
                    lock.release()
        finally:
            locallock.release()

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

    def changegroupsubset(self, bases, heads, source):
        """Compute a changegroup consisting of all the nodes that are
        descendants of any of the bases and ancestors of any of the heads.
        Return a chunkbuffer object whose read() method will return
        successive changegroup chunks.

        It is fairly complex as determining which filenodes and which
        manifest nodes need to be included for the changeset to be complete
        is non-trivial.

        Another wrinkle is doing the reverse, figuring out which changeset in
        the changegroup a particular filenode or manifestnode belongs to.
        """
        cl = self.changelog
        if not bases:
            bases = [nullid]
        csets, bases, heads = cl.nodesbetween(bases, heads)
        # We assume that all ancestors of bases are known
        common = set(cl.ancestors(*[cl.rev(n) for n in bases]))
        return self._changegroupsubset(common, csets, heads, source)

    def getlocalbundle(self, source, outgoing):
        """Like getbundle, but taking a discovery.outgoing as an argument.

        This is only implemented for local repos and reuses potentially
        precomputed sets in outgoing."""
        if not outgoing.missing:
            return None
        return self._changegroupsubset(outgoing.common,
                                       outgoing.missing,
                                       outgoing.missingheads,
                                       source)

    def getbundle(self, source, heads=None, common=None):
        """Like changegroupsubset, but returns the set difference between the
        ancestors of heads and the ancestors common.

        If heads is None, use the local heads. If common is None, use [nullid].

        The nodes in common might not all be known locally due to the way the
        current discovery protocol works.
        """
        cl = self.changelog
        if common:
            nm = cl.nodemap
            common = [n for n in common if n in nm]
        else:
            common = [nullid]
        if not heads:
            heads = cl.heads()
        return self.getlocalbundle(source,
                                   discovery.outgoing(cl, common, heads))

    def _changegroupsubset(self, commonrevs, csets, heads, source):

        cl = self.changelog
        mf = self.manifest
        mfs = {} # needed manifests
        fnodes = {} # needed file nodes
        changedfiles = set()
        fstate = ['', {}]
        count = [0, 0]

        # can we go through the fast path ?
        heads.sort()
        if heads == sorted(self.heads()):
            return self._changegroup(csets, source)

        # slow path
        self.hook('preoutgoing', throw=True, source=source)
        self.changegroupinfo(csets, source)

        # filter any nodes that claim to be part of the known set
        def prune(revlog, missing):
            rr, rl = revlog.rev, revlog.linkrev
            return [n for n in missing
                    if rl(rr(n)) not in commonrevs]

        progress = self.ui.progress
        _bundling = _('bundling')
        _changesets = _('changesets')
        _manifests = _('manifests')
        _files = _('files')

        def lookup(revlog, x):
            if revlog == cl:
                c = cl.read(x)
                changedfiles.update(c[3])
                mfs.setdefault(c[0], x)
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_changesets, total=count[1])
                return x
            elif revlog == mf:
                clnode = mfs[x]
                mdata = mf.readfast(x)
                for f, n in mdata.iteritems():
                    if f in changedfiles:
                        fnodes[f].setdefault(n, clnode)
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_manifests, total=count[1])
                return clnode
            else:
                progress(_bundling, count[0], item=fstate[0],
                         unit=_files, total=count[1])
                return fstate[1][x]

        bundler = changegroup.bundle10(lookup)
        reorder = self.ui.config('bundle', 'reorder', 'auto')
        if reorder == 'auto':
            reorder = None
        else:
            reorder = util.parsebool(reorder)

        def gengroup():
            # Create a changenode group generator that will call our functions
            # back to lookup the owning changenode and collect information.
            count[:] = [0, len(csets)]
            for chunk in cl.group(csets, bundler, reorder=reorder):
                yield chunk
            progress(_bundling, None)

            # Create a generator for the manifestnodes that calls our lookup
            # and data collection functions back.
            for f in changedfiles:
                fnodes[f] = {}
            count[:] = [0, len(mfs)]
            for chunk in mf.group(prune(mf, mfs), bundler, reorder=reorder):
                yield chunk
            progress(_bundling, None)

            mfs.clear()

            # Go through all our files in order sorted by name.
            count[:] = [0, len(changedfiles)]
            for fname in sorted(changedfiles):
                filerevlog = self.file(fname)
                if not len(filerevlog):
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                fstate[0] = fname
                fstate[1] = fnodes.pop(fname, {})

                nodelist = prune(filerevlog, fstate[1])
                if nodelist:
                    count[0] += 1
                    yield bundler.fileheader(fname)
                    for chunk in filerevlog.group(nodelist, bundler, reorder):
                        yield chunk

            # Signal that no more groups are left.
            yield bundler.close()
            progress(_bundling, None)

            if csets:
                self.hook('outgoing', node=hex(csets[0]), source=source)

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

        cl = self.changelog
        mf = self.manifest
        mfs = {}
        changedfiles = set()
        fstate = ['']
        count = [0, 0]

        self.hook('preoutgoing', throw=True, source=source)
        self.changegroupinfo(nodes, source)

        revset = set([cl.rev(n) for n in nodes])

        def gennodelst(log):
            ln, llr = log.node, log.linkrev
            return [ln(r) for r in log if llr(r) in revset]

        progress = self.ui.progress
        _bundling = _('bundling')
        _changesets = _('changesets')
        _manifests = _('manifests')
        _files = _('files')

        def lookup(revlog, x):
            if revlog == cl:
                c = cl.read(x)
                changedfiles.update(c[3])
                mfs.setdefault(c[0], x)
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_changesets, total=count[1])
                return x
            elif revlog == mf:
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_manifests, total=count[1])
                return cl.node(revlog.linkrev(revlog.rev(x)))
            else:
                progress(_bundling, count[0], item=fstate[0],
                    total=count[1], unit=_files)
                return cl.node(revlog.linkrev(revlog.rev(x)))

        bundler = changegroup.bundle10(lookup)
        reorder = self.ui.config('bundle', 'reorder', 'auto')
        if reorder == 'auto':
            reorder = None
        else:
            reorder = util.parsebool(reorder)

        def gengroup():
            '''yield a sequence of changegroup chunks (strings)'''
            # construct a list of all changed files

            count[:] = [0, len(nodes)]
            for chunk in cl.group(nodes, bundler, reorder=reorder):
                yield chunk
            progress(_bundling, None)

            count[:] = [0, len(mfs)]
            for chunk in mf.group(gennodelst(mf), bundler, reorder=reorder):
                yield chunk
            progress(_bundling, None)

            count[:] = [0, len(changedfiles)]
            for fname in sorted(changedfiles):
                filerevlog = self.file(fname)
                if not len(filerevlog):
                    raise util.Abort(_("empty or missing revlog for %s") % fname)
                fstate[0] = fname
                nodelist = gennodelst(filerevlog)
                if nodelist:
                    count[0] += 1
                    yield bundler.fileheader(fname)
                    for chunk in filerevlog.group(nodelist, bundler, reorder):
                        yield chunk
            yield bundler.close()
            progress(_bundling, None)

            if nodes:
                self.hook('outgoing', node=hex(nodes[0]), source=source)

        return changegroup.unbundle10(util.chunkbuffer(gengroup()), 'UN')

    def addchangegroup(self, source, srctype, url, emptyok=False):
        """Add the changegroup returned by source.read() to this repo.
        srctype is a string like 'push', 'pull', or 'unbundle'.  url is
        the URL of the repo where this changegroup is coming from.

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
        oldheads = cl.heads()

        tr = self.transaction("\n".join([srctype, util.hidepassword(url)]))
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

            source.changelogheader()
            srccontent = cl.addgroup(source, csmap, trp)
            if not (srccontent or emptyok):
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
            source.manifestheader()
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
            pr.step = _('files')
            pr.count = 1
            pr.total = efiles
            source.callback = None

            while True:
                chunkdata = source.filelogheader()
                if not chunkdata:
                    break
                f = chunkdata["filename"]
                self.ui.debug("adding %s revisions\n" % f)
                pr()
                fl = self.file(f)
                o = len(fl)
                if not fl.addgroup(source, revmap, trp):
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

            dh = 0
            if oldheads:
                heads = cl.heads()
                dh = len(heads) - len(oldheads)
                for h in heads:
                    if h not in oldheads and 'close' in self[h].extra():
                        dh -= 1
            htext = ""
            if dh:
                htext = _(" (%+d heads)") % dh

            self.ui.status(_("added %d changesets"
                             " with %d changes to %d files%s\n")
                             % (changesets, revisions, files, htext))

            if changesets > 0:
                p = lambda: cl.writepending() and self.root or ""
                self.hook('pretxnchangegroup', throw=True,
                          node=hex(cl.node(clstart)), source=srctype,
                          url=url, pending=p)

            added = [cl.node(r) for r in xrange(clstart, clend)]
            publishing = self.ui.configbool('phases', 'publish', True)
            if srctype == 'push':
                # Old server can not push the boundary themself.
                # New server won't push the boundary if changeset already
                # existed locally as secrete
                #
                # We should not use added here but the list of all change in
                # the bundle
                if publishing:
                    phases.advanceboundary(self, phases.public, srccontent)
                else:
                    phases.advanceboundary(self, phases.draft, srccontent)
                    phases.retractboundary(self, phases.draft, added)
            elif srctype != 'strip':
                # publishing only alter behavior during push
                #
                # strip should not touch boundary at all
                phases.retractboundary(self, phases.draft, added)

            # make changelog see real files again
            cl.finalize(trp)

            tr.close()

            if changesets > 0:
                def runhooks():
                    # forcefully update the on-disk branch cache
                    self.ui.debug("updating the branch cache\n")
                    self.updatebranchcache()
                    self.hook("changegroup", node=hex(cl.node(clstart)),
                              source=srctype, url=url)

                    for n in added:
                        self.hook("incoming", node=hex(n), source=srctype,
                                  url=url)
                self._afterlock(runhooks)

        finally:
            tr.release()
        # never return 0 here:
        if dh < 0:
            return dh - 1
        else:
            return dh + 1

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
                if self.ui.debugflag:
                    self.ui.debug('adding %s (%s)\n' %
                                  (name, util.bytecount(size)))
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

        if not stream:
            # if the server explicitely prefer to stream (for fast LANs)
            stream = remote.capable('stream-preferred')

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
        self.hook('prepushkey', throw=True, namespace=namespace, key=key,
                  old=old, new=new)
        ret = pushkey.push(self, namespace, key, old, new)
        self.hook('pushkey', namespace=namespace, key=key, old=old, new=new,
                  ret=ret)
        return ret

    def listkeys(self, namespace):
        self.hook('prelistkeys', throw=True, namespace=namespace)
        values = pushkey.list(self, namespace)
        self.hook('listkeys', namespace=namespace, values=values)
        return values

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        '''used to test argument passing over the wire'''
        return "%s %s %s %s %s" % (one, two, three, four, five)

    def savecommitmessage(self, text):
        fp = self.opener('last-message.txt', 'wb')
        try:
            fp.write(text)
        finally:
            fp.close()
        return self.pathto(fp.name[len(self.root)+1:])

# used to avoid circular references so destructors work
def aftertrans(files):
    renamefiles = [tuple(t) for t in files]
    def a():
        for src, dest in renamefiles:
            try:
                util.rename(src, dest)
            except OSError: # journal file does not yet exist
                pass
    return a

def undoname(fn):
    base, name = os.path.split(fn)
    assert name.startswith('journal')
    return os.path.join(base, name.replace('journal', 'undo', 1))

def instance(ui, path, create):
    return localrepository(ui, util.urllocalpath(path), create)

def islocal(path):
    return True
