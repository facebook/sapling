# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from node import hex, nullid, short
from i18n import _
import urllib
import peer, changegroup, subrepo, pushkey, obsolete, repoview
import changelog, dirstate, filelog, manifest, context, bookmarks, phases
import lock as lockmod
import transaction, store, encoding, exchange, bundle2
import scmutil, util, extensions, hook, error, revset
import match as matchmod
import merge as mergemod
import tags as tagsmod
from lock import release
import weakref, errno, os, time, inspect, random
import branchmap, pathutil
import namespaces
propertycache = util.propertycache
filecache = scmutil.filecache

class repofilecache(filecache):
    """All filecache usage on repo are done for logic that should be unfiltered
    """

    def __get__(self, repo, type=None):
        return super(repofilecache, self).__get__(repo.unfiltered(), type)
    def __set__(self, repo, value):
        return super(repofilecache, self).__set__(repo.unfiltered(), value)
    def __delete__(self, repo):
        return super(repofilecache, self).__delete__(repo.unfiltered())

class storecache(repofilecache):
    """filecache for files in the store"""
    def join(self, obj, fname):
        return obj.sjoin(fname)

class unfilteredpropertycache(propertycache):
    """propertycache that apply to unfiltered repo only"""

    def __get__(self, repo, type=None):
        unfi = repo.unfiltered()
        if unfi is repo:
            return super(unfilteredpropertycache, self).__get__(unfi)
        return getattr(unfi, self.name)

class filteredpropertycache(propertycache):
    """propertycache that must take filtering in account"""

    def cachevalue(self, obj, value):
        object.__setattr__(obj, self.name, value)


def hasunfilteredcache(repo, name):
    """check if a repo has an unfilteredpropertycache value for <name>"""
    return name in vars(repo.unfiltered())

def unfilteredmethod(orig):
    """decorate method that always need to be run on unfiltered version"""
    def wrapper(repo, *args, **kwargs):
        return orig(repo.unfiltered(), *args, **kwargs)
    return wrapper

moderncaps = set(('lookup', 'branchmap', 'pushkey', 'known', 'getbundle',
                  'unbundle'))
legacycaps = moderncaps.union(set(['changegroupsubset']))

class localpeer(peer.peerrepository):
    '''peer for a local repo; reflects only the most recent API'''

    def __init__(self, repo, caps=moderncaps):
        peer.peerrepository.__init__(self)
        self._repo = repo.filtered('served')
        self.ui = repo.ui
        self._caps = repo._restrictcapabilities(caps)
        self.requirements = repo.requirements
        self.supportedformats = repo.supportedformats

    def close(self):
        self._repo.close()

    def _capabilities(self):
        return self._caps

    def local(self):
        return self._repo

    def canpush(self):
        return True

    def url(self):
        return self._repo.url()

    def lookup(self, key):
        return self._repo.lookup(key)

    def branchmap(self):
        return self._repo.branchmap()

    def heads(self):
        return self._repo.heads()

    def known(self, nodes):
        return self._repo.known(nodes)

    def getbundle(self, source, heads=None, common=None, bundlecaps=None,
                  **kwargs):
        cg = exchange.getbundle(self._repo, source, heads=heads,
                                common=common, bundlecaps=bundlecaps, **kwargs)
        if bundlecaps is not None and 'HG20' in bundlecaps:
            # When requesting a bundle2, getbundle returns a stream to make the
            # wire level function happier. We need to build a proper object
            # from it in local peer.
            cg = bundle2.getunbundler(self.ui, cg)
        return cg

    # TODO We might want to move the next two calls into legacypeer and add
    # unbundle instead.

    def unbundle(self, cg, heads, url):
        """apply a bundle on a repo

        This function handles the repo locking itself."""
        try:
            try:
                cg = exchange.readbundle(self.ui, cg, None)
                ret = exchange.unbundle(self._repo, cg, heads, 'push', url)
                if util.safehasattr(ret, 'getchunks'):
                    # This is a bundle20 object, turn it into an unbundler.
                    # This little dance should be dropped eventually when the
                    # API is finally improved.
                    stream = util.chunkbuffer(ret.getchunks())
                    ret = bundle2.getunbundler(self.ui, stream)
                return ret
            except Exception as exc:
                # If the exception contains output salvaged from a bundle2
                # reply, we need to make sure it is printed before continuing
                # to fail. So we build a bundle2 with such output and consume
                # it directly.
                #
                # This is not very elegant but allows a "simple" solution for
                # issue4594
                output = getattr(exc, '_bundle2salvagedoutput', ())
                if output:
                    bundler = bundle2.bundle20(self._repo.ui)
                    for out in output:
                        bundler.addpart(out)
                    stream = util.chunkbuffer(bundler.getchunks())
                    b = bundle2.getunbundler(self.ui, stream)
                    bundle2.processbundle(self._repo, b)
                raise
        except error.PushRaced as exc:
            raise error.ResponseError(_('push failed:'), str(exc))

    def lock(self):
        return self._repo.lock()

    def addchangegroup(self, cg, source, url):
        return changegroup.addchangegroup(self._repo, cg, source, url)

    def pushkey(self, namespace, key, old, new):
        return self._repo.pushkey(namespace, key, old, new)

    def listkeys(self, namespace):
        return self._repo.listkeys(namespace)

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        '''used to test argument passing over the wire'''
        return "%s %s %s %s %s" % (one, two, three, four, five)

class locallegacypeer(localpeer):
    '''peer extension which implements legacy methods too; used for tests with
    restricted capabilities'''

    def __init__(self, repo):
        localpeer.__init__(self, repo, caps=legacycaps)

    def branches(self, nodes):
        return self._repo.branches(nodes)

    def between(self, pairs):
        return self._repo.between(pairs)

    def changegroup(self, basenodes, source):
        return changegroup.changegroup(self._repo, basenodes, source)

    def changegroupsubset(self, bases, heads, source):
        return changegroup.changegroupsubset(self._repo, bases, heads, source)

class localrepository(object):

    supportedformats = set(('revlogv1', 'generaldelta', 'treemanifest',
                            'manifestv2'))
    _basesupported = supportedformats | set(('store', 'fncache', 'shared',
                                             'dotencode'))
    openerreqs = set(('revlogv1', 'generaldelta', 'treemanifest', 'manifestv2'))
    filtername = None

    # a list of (ui, featureset) functions.
    # only functions defined in module of enabled extensions are invoked
    featuresetupfuncs = set()

    def _baserequirements(self, create):
        return ['revlogv1']

    def __init__(self, baseui, path=None, create=False):
        self.requirements = set()
        self.wvfs = scmutil.vfs(path, expandpath=True, realpath=True)
        self.wopener = self.wvfs
        self.root = self.wvfs.base
        self.path = self.wvfs.join(".hg")
        self.origroot = path
        self.auditor = pathutil.pathauditor(self.root, self._checknested)
        self.vfs = scmutil.vfs(self.path)
        self.opener = self.vfs
        self.baseui = baseui
        self.ui = baseui.copy()
        self.ui.copy = baseui.copy # prevent copying repo configuration
        # A list of callback to shape the phase if no data were found.
        # Callback are in the form: func(repo, roots) --> processed root.
        # This list it to be filled by extension during repo setup
        self._phasedefaults = []
        try:
            self.ui.readconfig(self.join("hgrc"), self.root)
            extensions.loadall(self.ui)
        except IOError:
            pass

        if self.featuresetupfuncs:
            self.supported = set(self._basesupported) # use private copy
            extmods = set(m.__name__ for n, m
                          in extensions.extensions(self.ui))
            for setupfunc in self.featuresetupfuncs:
                if setupfunc.__module__ in extmods:
                    setupfunc(self.ui, self.supported)
        else:
            self.supported = self._basesupported

        if not self.vfs.isdir():
            if create:
                if not self.wvfs.exists():
                    self.wvfs.makedirs()
                self.vfs.makedir(notindexed=True)
                self.requirements.update(self._baserequirements(create))
                if self.ui.configbool('format', 'usestore', True):
                    self.vfs.mkdir("store")
                    self.requirements.add("store")
                    if self.ui.configbool('format', 'usefncache', True):
                        self.requirements.add("fncache")
                        if self.ui.configbool('format', 'dotencode', True):
                            self.requirements.add('dotencode')
                    # create an invalid changelog
                    self.vfs.append(
                        "00changelog.i",
                        '\0\0\0\2' # represents revlogv2
                        ' dummy changelog to prevent using the old repo layout'
                    )
                if self.ui.configbool('format', 'generaldelta', False):
                    self.requirements.add("generaldelta")
                if self.ui.configbool('experimental', 'treemanifest', False):
                    self.requirements.add("treemanifest")
                if self.ui.configbool('experimental', 'manifestv2', False):
                    self.requirements.add("manifestv2")
            else:
                raise error.RepoError(_("repository %s not found") % path)
        elif create:
            raise error.RepoError(_("repository %s already exists") % path)
        else:
            try:
                self.requirements = scmutil.readrequires(
                        self.vfs, self.supported)
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise

        self.sharedpath = self.path
        try:
            vfs = scmutil.vfs(self.vfs.read("sharedpath").rstrip('\n'),
                              realpath=True)
            s = vfs.base
            if not vfs.exists():
                raise error.RepoError(
                    _('.hg/sharedpath points to nonexistent directory %s') % s)
            self.sharedpath = s
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise

        self.store = store.store(
                self.requirements, self.sharedpath, scmutil.vfs)
        self.spath = self.store.path
        self.svfs = self.store.vfs
        self.sopener = self.svfs
        self.sjoin = self.store.join
        self.vfs.createmode = self.store.createmode
        self._applyopenerreqs()
        if create:
            self._writerequirements()


        self._branchcaches = {}
        self._revbranchcache = None
        self.filterpats = {}
        self._datafilters = {}
        self._transref = self._lockref = self._wlockref = None

        # A cache for various files under .hg/ that tracks file changes,
        # (used by the filecache decorator)
        #
        # Maps a property name to its util.filecacheentry
        self._filecache = {}

        # hold sets of revision to be filtered
        # should be cleared when something might have changed the filter value:
        # - new changesets,
        # - phase change,
        # - new obsolescence marker,
        # - working directory parent change,
        # - bookmark changes
        self.filteredrevcache = {}

        # generic mapping between names and nodes
        self.names = namespaces.namespaces()

    def close(self):
        self._writecaches()

    def _writecaches(self):
        if self._revbranchcache:
            self._revbranchcache.write()

    def _restrictcapabilities(self, caps):
        if self.ui.configbool('experimental', 'bundle2-advertise', True):
            caps = set(caps)
            capsblob = bundle2.encodecaps(bundle2.getrepocaps(self))
            caps.add('bundle2=' + urllib.quote(capsblob))
        return caps

    def _applyopenerreqs(self):
        self.svfs.options = dict((r, 1) for r in self.requirements
                                           if r in self.openerreqs)
        chunkcachesize = self.ui.configint('format', 'chunkcachesize')
        if chunkcachesize is not None:
            self.svfs.options['chunkcachesize'] = chunkcachesize
        maxchainlen = self.ui.configint('format', 'maxchainlen')
        if maxchainlen is not None:
            self.svfs.options['maxchainlen'] = maxchainlen
        manifestcachesize = self.ui.configint('format', 'manifestcachesize')
        if manifestcachesize is not None:
            self.svfs.options['manifestcachesize'] = manifestcachesize

    def _writerequirements(self):
        scmutil.writerequires(self.vfs, self.requirements)

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

    def peer(self):
        return localpeer(self) # not cached to avoid reference cycle

    def unfiltered(self):
        """Return unfiltered version of the repository

        Intended to be overwritten by filtered repo."""
        return self

    def filtered(self, name):
        """Return a filtered version of a repository"""
        # build a new class with the mixin and the current class
        # (possibly subclass of the repo)
        class proxycls(repoview.repoview, self.unfiltered().__class__):
            pass
        return proxycls(self, name)

    @repofilecache('bookmarks')
    def _bookmarks(self):
        return bookmarks.bmstore(self)

    @repofilecache('bookmarks.current')
    def _activebookmark(self):
        return bookmarks.readactive(self)

    def bookmarkheads(self, bookmark):
        name = bookmark.split('@', 1)[0]
        heads = []
        for mark, n in self._bookmarks.iteritems():
            if mark.split('@', 1)[0] == name:
                heads.append(n)
        return heads

    @storecache('phaseroots')
    def _phasecache(self):
        return phases.phasecache(self, self._phasedefaults)

    @storecache('obsstore')
    def obsstore(self):
        # read default format for new obsstore.
        defaultformat = self.ui.configint('format', 'obsstore-version', None)
        # rely on obsstore class default when possible.
        kwargs = {}
        if defaultformat is not None:
            kwargs['defaultformat'] = defaultformat
        readonly = not obsolete.isenabled(self, obsolete.createmarkersopt)
        store = obsolete.obsstore(self.svfs, readonly=readonly,
                                  **kwargs)
        if store and readonly:
            self.ui.warn(
                _('obsolete feature not enabled but %i markers found!\n')
                % len(list(store)))
        return store

    @storecache('00changelog.i')
    def changelog(self):
        c = changelog.changelog(self.svfs)
        if 'HG_PENDING' in os.environ:
            p = os.environ['HG_PENDING']
            if p.startswith(self.root):
                c.readpending('00changelog.i.a')
        return c

    @storecache('00manifest.i')
    def manifest(self):
        return manifest.manifest(self.svfs)

    def dirlog(self, dir):
        return self.manifest.dirlog(dir)

    @repofilecache('dirstate')
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

        return dirstate.dirstate(self.vfs, self.ui, self.root, validate)

    def __getitem__(self, changeid):
        if changeid is None:
            return context.workingctx(self)
        if isinstance(changeid, slice):
            return [context.changectx(self, i)
                    for i in xrange(*changeid.indices(len(self)))
                    if i not in self.changelog.filteredrevs]
        return context.changectx(self, changeid)

    def __contains__(self, changeid):
        try:
            self[changeid]
            return True
        except error.RepoLookupError:
            return False

    def __nonzero__(self):
        return True

    def __len__(self):
        return len(self.changelog)

    def __iter__(self):
        return iter(self.changelog)

    def revs(self, expr, *args):
        '''Return a list of revisions matching the given revset'''
        expr = revset.formatspec(expr, *args)
        m = revset.match(None, expr)
        return m(self)

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
        """Call a hook, passing this repo instance.

        This a convenience method to aid invoking hooks. Extensions likely
        won't call this unless they have registered a custom hook or are
        replacing code that is expected to call a hook.
        """
        return hook.hook(self.ui, self, name, throw, **args)

    @unfilteredmethod
    def _tag(self, names, node, message, local, user, date, extra={},
             editor=False):
        if isinstance(names, str):
            names = (names,)

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
                if munge:
                    m = munge(name)
                else:
                    m = name

                if (self._tagscache.tagtypes and
                    name in self._tagscache.tagtypes):
                    old = self.tags().get(name, nullid)
                    fp.write('%s %s\n' % (hex(old), m))
                fp.write('%s %s\n' % (hex(node), m))
            fp.close()

        prevtags = ''
        if local:
            try:
                fp = self.vfs('localtags', 'r+')
            except IOError:
                fp = self.vfs('localtags', 'a')
            else:
                prevtags = fp.read()

            # local tags are stored in the current charset
            writetags(fp, names, None, prevtags)
            for name in names:
                self.hook('tag', node=hex(node), tag=name, local=local)
            return

        try:
            fp = self.wfile('.hgtags', 'rb+')
        except IOError as e:
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
        tagnode = self.commit(message, user, date, extra=extra, match=m,
                              editor=editor)

        for name in names:
            self.hook('tag', node=hex(node), tag=name, local=local)

        return tagnode

    def tag(self, names, node, message, local, user, date, editor=False):
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
            m = matchmod.exact(self.root, '', ['.hgtags'])
            if any(self.status(match=m, unknown=True, ignored=True)):
                raise util.Abort(_('working copy of .hgtags is changed'),
                                 hint=_('please commit .hgtags manually'))

        self.tags() # instantiate the cache
        self._tag(names, node, message, local, user, date, editor=editor)

    @filteredpropertycache
    def _tagscache(self):
        '''Returns a tagscache object that contains various tags related
        caches.'''

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
        if self.changelog.filteredrevs:
            tags, tt = self._findtags()
        else:
            tags = self._tagscache.tags
        for k, v in tags.iteritems():
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
                l.append((self.changelog.rev(n), t, n))
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

    def branchmap(self):
        '''returns a dictionary {branch: [branchheads]} with branchheads
        ordered by increasing revision number'''
        branchmap.updatecache(self)
        return self._branchcaches[self.filtername]

    @unfilteredmethod
    def revbranchcache(self):
        if not self._revbranchcache:
            self._revbranchcache = branchmap.revbranchcache(self.unfiltered())
        return self._revbranchcache

    def branchtip(self, branch, ignoremissing=False):
        '''return the tip node for a given branch

        If ignoremissing is True, then this method will not raise an error.
        This is helpful for callers that only expect None for a missing branch
        (e.g. namespace).

        '''
        try:
            return self.branchmap().branchtip(branch)
        except KeyError:
            if not ignoremissing:
                raise error.RepoLookupError(_("unknown branch '%s'") % branch)
            else:
                pass

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
        pc = self._phasecache
        result = []
        for n in nodes:
            r = nm.get(n)
            resp = not (r is None or pc.phase(self, r) >= phases.secret)
            result.append(resp)
        return result

    def local(self):
        return self

    def publishing(self):
        # it's safe (and desirable) to trust the publish flag unconditionally
        # so that we don't finalize changes shared between users via ssh or nfs
        return self.ui.configbool('phases', 'publish', True, untrusted=True)

    def cancopy(self):
        # so statichttprepo's override of local() works
        if not self.local():
            return False
        if not self.publishing():
            return True
        # if publishing we can't copy if there is filtered content
        return not self.filtered('visible').changelog.filteredrevs

    def shared(self):
        '''the type of shared repository (None if not shared)'''
        if self.sharedpath != self.path:
            return 'store'
        return None

    def join(self, f, *insidef):
        return self.vfs.join(os.path.join(f, *insidef))

    def wjoin(self, f, *insidef):
        return self.vfs.reljoin(self.root, f, *insidef)

    def file(self, f):
        if f[0] == '/':
            f = f[1:]
        return filelog.filelog(self.svfs, f)

    def changectx(self, changeid):
        return self[changeid]

    def parents(self, changeid=None):
        '''get list of changectxs for parents of changeid'''
        return self[changeid].parents()

    def setparents(self, p1, p2=nullid):
        self.dirstate.beginparentchange()
        copies = self.dirstate.setparents(p1, p2)
        pctx = self[p1]
        if copies:
            # Adjust copy records, the dirstate cannot do it, it
            # requires access to parents manifests. Preserve them
            # only for entries added to first parent.
            for f in copies:
                if f not in pctx and copies[f] in pctx:
                    self.dirstate.copy(copies[f], f)
        if p2 == nullid:
            for f, s in sorted(self.dirstate.copies().items()):
                if f not in pctx and s not in pctx:
                    self.dirstate.copy(None, f)
        self.dirstate.endparentchange()

    def filectx(self, path, changeid=None, fileid=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        return context.filectx(self, path, changeid, fileid)

    def getcwd(self):
        return self.dirstate.getcwd()

    def pathto(self, f, cwd=None):
        return self.dirstate.pathto(f, cwd)

    def wfile(self, f, mode='r'):
        return self.wvfs(f, mode)

    def _link(self, f):
        return self.wvfs.islink(f)

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

    @unfilteredpropertycache
    def _encodefilterpats(self):
        return self._loadfilter('encode')

    @unfilteredpropertycache
    def _decodefilterpats(self):
        return self._loadfilter('decode')

    def adddatafilter(self, name, filter):
        self._datafilters[name] = filter

    def wread(self, filename):
        if self._link(filename):
            data = self.wvfs.readlink(filename)
        else:
            data = self.wvfs.read(filename)
        return self._filter(self._encodefilterpats, filename, data)

    def wwrite(self, filename, data, flags):
        """write ``data`` into ``filename`` in the working directory

        This returns length of written (maybe decoded) data.
        """
        data = self._filter(self._decodefilterpats, filename, data)
        if 'l' in flags:
            self.wvfs.symlink(data, filename)
        else:
            self.wvfs.write(filename, data)
            if 'x' in flags:
                self.wvfs.setflags(filename, False, True)
        return len(data)

    def wwritedata(self, filename, data):
        return self._filter(self._decodefilterpats, filename, data)

    def currenttransaction(self):
        """return the current transaction or None if non exists"""
        if self._transref:
            tr = self._transref()
        else:
            tr = None

        if tr and tr.running():
            return tr
        return None

    def transaction(self, desc, report=None):
        if (self.ui.configbool('devel', 'all-warnings')
                or self.ui.configbool('devel', 'check-locks')):
            l = self._lockref and self._lockref()
            if l is None or not l.held:
                self.ui.develwarn('transaction with no lock')
        tr = self.currenttransaction()
        if tr is not None:
            return tr.nest()

        # abort here if the journal already exists
        if self.svfs.exists("journal"):
            raise error.RepoError(
                _("abandoned transaction found"),
                hint=_("run 'hg recover' to clean up transaction"))

        idbase = "%.40f#%f" % (random.random(), time.time())
        txnid = 'TXN:' + util.sha1(idbase).hexdigest()
        self.hook('pretxnopen', throw=True, txnname=desc, txnid=txnid)

        self._writejournal(desc)
        renames = [(vfs, x, undoname(x)) for vfs, x in self._journalfiles()]
        if report:
            rp = report
        else:
            rp = self.ui.warn
        vfsmap = {'plain': self.vfs} # root of .hg/
        # we must avoid cyclic reference between repo and transaction.
        reporef = weakref.ref(self)
        def validate(tr):
            """will run pre-closing hooks"""
            pending = lambda: tr.writepending() and self.root or ""
            reporef().hook('pretxnclose', throw=True, pending=pending,
                           txnname=desc, **tr.hookargs)

        tr = transaction.transaction(rp, self.sopener, vfsmap,
                                     "journal",
                                     "undo",
                                     aftertrans(renames),
                                     self.store.createmode,
                                     validator=validate)

        tr.hookargs['txnid'] = txnid
        # note: writing the fncache only during finalize mean that the file is
        # outdated when running hooks. As fncache is used for streaming clone,
        # this is not expected to break anything that happen during the hooks.
        tr.addfinalize('flush-fncache', self.store.write)
        def txnclosehook(tr2):
            """To be run if transaction is successful, will schedule a hook run
            """
            def hook():
                reporef().hook('txnclose', throw=False, txnname=desc,
                               **tr2.hookargs)
            reporef()._afterlock(hook)
        tr.addfinalize('txnclose-hook', txnclosehook)
        def txnaborthook(tr2):
            """To be run if transaction is aborted
            """
            reporef().hook('txnabort', throw=False, txnname=desc,
                           **tr2.hookargs)
        tr.addabort('txnabort-hook', txnaborthook)
        self._transref = weakref.ref(tr)
        return tr

    def _journalfiles(self):
        return ((self.svfs, 'journal'),
                (self.vfs, 'journal.dirstate'),
                (self.vfs, 'journal.branch'),
                (self.vfs, 'journal.desc'),
                (self.vfs, 'journal.bookmarks'),
                (self.svfs, 'journal.phaseroots'))

    def undofiles(self):
        return [(vfs, undoname(x)) for vfs, x in self._journalfiles()]

    def _writejournal(self, desc):
        self.vfs.write("journal.dirstate",
                          self.vfs.tryread("dirstate"))
        self.vfs.write("journal.branch",
                          encoding.fromlocal(self.dirstate.branch()))
        self.vfs.write("journal.desc",
                          "%d\n%s\n" % (len(self), desc))
        self.vfs.write("journal.bookmarks",
                          self.vfs.tryread("bookmarks"))
        self.svfs.write("journal.phaseroots",
                           self.svfs.tryread("phaseroots"))

    def recover(self):
        lock = self.lock()
        try:
            if self.svfs.exists("journal"):
                self.ui.status(_("rolling back interrupted transaction\n"))
                vfsmap = {'': self.svfs,
                          'plain': self.vfs,}
                transaction.rollback(self.svfs, vfsmap, "journal",
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
            if self.svfs.exists("undo"):
                return self._rollback(dryrun, force)
            else:
                self.ui.warn(_("no rollback information available\n"))
                return 1
        finally:
            release(lock, wlock)

    @unfilteredmethod # Until we get smarter cache management
    def _rollback(self, dryrun, force):
        ui = self.ui
        try:
            args = self.vfs.read('undo.desc').splitlines()
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
        self.destroying()
        vfsmap = {'plain': self.vfs, '': self.svfs}
        transaction.rollback(self.svfs, vfsmap, 'undo', ui.warn)
        if self.vfs.exists('undo.bookmarks'):
            self.vfs.rename('undo.bookmarks', 'bookmarks')
        if self.svfs.exists('undo.phaseroots'):
            self.svfs.rename('undo.phaseroots', 'phaseroots')
        self.invalidate()

        parentgone = (parents[0] not in self.changelog.nodemap or
                      parents[1] not in self.changelog.nodemap)
        if parentgone:
            self.vfs.rename('undo.dirstate', 'dirstate')
            try:
                branch = self.vfs.read('undo.branch')
                self.dirstate.setbranch(encoding.tolocal(branch))
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
            ms = mergemod.mergestate(self)
            ms.reset(self['.'].node())

        # TODO: if we know which new heads may result from this rollback, pass
        # them to destroy(), which will prevent the branchhead cache from being
        # invalidated.
        self.destroyed()
        return 0

    def invalidatecaches(self):

        if '_tagscache' in vars(self):
            # can't use delattr on proxy
            del self.__dict__['_tagscache']

        self.unfiltered()._branchcaches.clear()
        self.invalidatevolatilesets()

    def invalidatevolatilesets(self):
        self.filteredrevcache.clear()
        obsolete.clearobscaches(self)

    def invalidatedirstate(self):
        '''Invalidates the dirstate, causing the next call to dirstate
        to check if it was modified since the last time it was read,
        rereading it if it has.

        This is different to dirstate.invalidate() that it doesn't always
        rereads the dirstate. Use dirstate.invalidate() if you want to
        explicitly read the dirstate again (i.e. restoring it to a previous
        known good state).'''
        if hasunfilteredcache(self, 'dirstate'):
            for k in self.dirstate._filecache:
                try:
                    delattr(self.dirstate, k)
                except AttributeError:
                    pass
            delattr(self.unfiltered(), 'dirstate')

    def invalidate(self):
        unfiltered = self.unfiltered() # all file caches are stored unfiltered
        for k in self._filecache:
            # dirstate is invalidated separately in invalidatedirstate()
            if k == 'dirstate':
                continue

            try:
                delattr(unfiltered, k)
            except AttributeError:
                pass
        self.invalidatecaches()
        self.store.invalidatecaches()

    def invalidateall(self):
        '''Fully invalidates both store and non-store parts, causing the
        subsequent operation to reread any outside changes.'''
        # extension should hook this to invalidate its caches
        self.invalidate()
        self.invalidatedirstate()

    def _lock(self, vfs, lockname, wait, releasefn, acquirefn, desc):
        try:
            l = lockmod.lock(vfs, lockname, 0, releasefn, desc=desc)
        except error.LockHeld as inst:
            if not wait:
                raise
            self.ui.warn(_("waiting for lock on %s held by %r\n") %
                         (desc, inst.locker))
            # default to 600 seconds timeout
            l = lockmod.lock(vfs, lockname,
                             int(self.ui.config("ui", "timeout", "600")),
                             releasefn, desc=desc)
            self.ui.warn(_("got lock after %s seconds\n") % l.delay)
        if acquirefn:
            acquirefn()
        return l

    def _afterlock(self, callback):
        """add a callback to be run when the repository is fully unlocked

        The callback will be executed when the outermost lock is released
        (with wlock being higher level than 'lock')."""
        for ref in (self._wlockref, self._lockref):
            l = ref and ref()
            if l and l.held:
                l.postrelease.append(callback)
                break
        else: # no lock have been found.
            callback()

    def lock(self, wait=True):
        '''Lock the repository store (.hg/store) and return a weak reference
        to the lock. Use this before modifying the store (e.g. committing or
        stripping). If you are opening a transaction, get a lock as well.)

        If both 'lock' and 'wlock' must be acquired, ensure you always acquires
        'wlock' first to avoid a dead-lock hazard.'''
        l = self._lockref and self._lockref()
        if l is not None and l.held:
            l.lock()
            return l

        def unlock():
            for k, ce in self._filecache.items():
                if k == 'dirstate' or k not in self.__dict__:
                    continue
                ce.refresh()

        l = self._lock(self.svfs, "lock", wait, unlock,
                       self.invalidate, _('repository %s') % self.origroot)
        self._lockref = weakref.ref(l)
        return l

    def wlock(self, wait=True):
        '''Lock the non-store parts of the repository (everything under
        .hg except .hg/store) and return a weak reference to the lock.

        Use this before modifying files in .hg.

        If both 'lock' and 'wlock' must be acquired, ensure you always acquires
        'wlock' first to avoid a dead-lock hazard.'''
        l = self._wlockref and self._wlockref()
        if l is not None and l.held:
            l.lock()
            return l

        # We do not need to check for non-waiting lock aquisition.  Such
        # acquisition would not cause dead-lock as they would just fail.
        if wait and (self.ui.configbool('devel', 'all-warnings')
                     or self.ui.configbool('devel', 'check-locks')):
            l = self._lockref and self._lockref()
            if l is not None and l.held:
                self.ui.develwarn('"wlock" acquired after "lock"')

        def unlock():
            if self.dirstate.pendingparentchange():
                self.dirstate.invalidate()
            else:
                self.dirstate.write()

            self._filecache['dirstate'].refresh()

        l = self._lock(self.vfs, "wlock", wait, unlock,
                       self.invalidatedirstate, _('working directory of %s') %
                       self.origroot)
        self._wlockref = weakref.ref(l)
        return l

    def _filecommit(self, fctx, manifest1, manifest2, linkrev, tr, changelist):
        """
        commit an individual file as part of a larger transaction
        """

        fname = fctx.path()
        fparent1 = manifest1.get(fname, nullid)
        fparent2 = manifest2.get(fname, nullid)
        if isinstance(fctx, context.filectx):
            node = fctx.filenode()
            if node in [fparent1, fparent2]:
                self.ui.debug('reusing %s filelog entry\n' % fname)
                return node

        flog = self.file(fname)
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

            # Here, we used to search backwards through history to try to find
            # where the file copy came from if the source of a copy was not in
            # the parent directory. However, this doesn't actually make sense to
            # do (what does a copy from something not in your working copy even
            # mean?) and it causes bugs (eg, issue4476). Instead, we will warn
            # the user that copy information was dropped, so if they didn't
            # expect this outcome it can be fixed, but this is the correct
            # behavior in this circumstance.

            if crev:
                self.ui.debug(" %s: copy %s:%s\n" % (fname, cfname, hex(crev)))
                meta["copy"] = cfname
                meta["copyrev"] = hex(crev)
                fparent1, fparent2 = nullid, newfparent
            else:
                self.ui.warn(_("warning: can't find ancestor for '%s' "
                               "copied from '%s'!\n") % (fname, cfname))

        elif fparent1 == nullid:
            fparent1, fparent2 = fparent2, nullid
        elif fparent2 != nullid:
            # is one parent an ancestor of the other?
            fparentancestors = flog.commonancestorsheads(fparent1, fparent2)
            if fparent1 in fparentancestors:
                fparent1, fparent2 = fparent2, nullid
            elif fparent2 in fparentancestors:
                fparent2 = nullid

        # is the file changed?
        text = fctx.data()
        if fparent2 != nullid or flog.cmp(fparent1, text) or meta:
            changelist.append(fname)
            return flog.add(text, meta, tr, linkrev, fparent1, fparent2)
        # are just the flags changed during merge?
        elif fname in manifest1 and manifest1.flags(fname) != fctx.flags():
            changelist.append(fname)

        return fparent1

    @unfilteredmethod
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
            match.explicitdir = vdirs.append
            match.bad = fail

        wlock = self.wlock()
        try:
            wctx = self[None]
            merge = len(wctx.parents()) > 1

            if not force and merge and match.ispartial():
                raise util.Abort(_('cannot partially commit a merge '
                                   '(do not specify files or patterns)'))

            status = self.status(match=match, clean=force)
            if force:
                status.modified.extend(status.clean) # mq may commit clean files

            # check subrepos
            subs = []
            commitsubs = set()
            newstate = wctx.substate.copy()
            # only manage subrepos and .hgsubstate if .hgsub is present
            if '.hgsub' in wctx:
                # we'll decide whether to track this ourselves, thanks
                for c in status.modified, status.added, status.removed:
                    if '.hgsubstate' in c:
                        c.remove('.hgsubstate')

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
                    dirtyreason = wctx.sub(s).dirtyreason(True)
                    if dirtyreason:
                        if not self.ui.configbool('ui', 'commitsubrepos'):
                            raise util.Abort(dirtyreason,
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
                    status.modified.insert(0, '.hgsubstate')

            elif '.hgsub' in status.removed:
                # clean up .hgsubstate when .hgsub is removed
                if ('.hgsubstate' in wctx and
                    '.hgsubstate' not in (status.modified + status.added +
                                          status.removed)):
                    status.removed.insert(0, '.hgsubstate')

            # make sure all explicit patterns are matched
            if not force and (match.isexact() or match.prefix()):
                matched = set(status.modified + status.added + status.removed)

                for f in match.files():
                    f = self.dirstate.normalize(f)
                    if f == '.' or f in matched or f in wctx.substate:
                        continue
                    if f in status.deleted:
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

            cctx = context.workingcommitctx(self, status,
                                            text, user, date, extra)

            allowemptycommit = (wctx.branch() != wctx.p1().branch()
                                or extra.get('close') or merge or cctx.files()
                                or self.ui.configbool('ui', 'allowemptycommit'))
            if not allowemptycommit:
                return None

            if merge and cctx.deleted():
                raise util.Abort(_("cannot commit merge with missing files"))

            ms = mergemod.mergestate(self)
            for f in status.modified:
                if f in ms and ms[f] == 'u':
                    raise util.Abort(_('unresolved merge conflicts '
                                       '(see "hg help resolve")'))

            if editor:
                cctx._text = editor(self, cctx, subs)
            edited = (text != cctx._text)

            # Save commit message in case this transaction gets rolled back
            # (e.g. by a pretxncommit hook).  Leave the content alone on
            # the assumption that the user will use the same editor again.
            msgfn = self.savecommitmessage(cctx._text)

            # commit subs and write new state
            if subs:
                for s in sorted(commitsubs):
                    sub = wctx.sub(s)
                    self.ui.status(_('committing subrepository %s\n') %
                        subrepo.subrelpath(sub))
                    sr = sub.commit(cctx._text, user, date)
                    newstate[s] = (newstate[s][0], sr)
                subrepo.writestate(self, newstate)

            p1, p2 = self.dirstate.parents()
            hookp1, hookp2 = hex(p1), (p2 != nullid and hex(p2) or '')
            try:
                self.hook("precommit", throw=True, parent1=hookp1,
                          parent2=hookp2)
                ret = self.commitctx(cctx, True)
            except: # re-raises
                if edited:
                    self.ui.write(
                        _('note: commit message saved in %s\n') % msgfn)
                raise

            # update bookmarks, dirstate and mergestate
            bookmarks.update(self, [p1, p2], ret)
            cctx.markcommitted(ret)
            ms.reset()
        finally:
            wlock.release()

        def commithook(node=hex(ret), parent1=hookp1, parent2=hookp2):
            # hack for command that use a temporary commit (eg: histedit)
            # temporary commit got stripped before hook release
            if self.changelog.hasnode(ret):
                self.hook("commit", node=node, parent1=parent1,
                          parent2=parent2)
        self._afterlock(commithook)
        return ret

    @unfilteredmethod
    def commitctx(self, ctx, error=False):
        """Add a new revision to current repository.
        Revision information is passed via the context argument.
        """

        tr = None
        p1, p2 = ctx.p1(), ctx.p2()
        user = ctx.user()

        lock = self.lock()
        try:
            tr = self.transaction("commit")
            trp = weakref.proxy(tr)

            if ctx.files():
                m1 = p1.manifest()
                m2 = p2.manifest()
                m = m1.copy()

                # check in files
                added = []
                changed = []
                removed = list(ctx.removed())
                linkrev = len(self)
                self.ui.note(_("committing files:\n"))
                for f in sorted(ctx.modified() + ctx.added()):
                    self.ui.note(f + "\n")
                    try:
                        fctx = ctx[f]
                        if fctx is None:
                            removed.append(f)
                        else:
                            added.append(f)
                            m[f] = self._filecommit(fctx, m1, m2, linkrev,
                                                    trp, changed)
                            m.setflag(f, fctx.flags())
                    except OSError as inst:
                        self.ui.warn(_("trouble committing %s!\n") % f)
                        raise
                    except IOError as inst:
                        errcode = getattr(inst, 'errno', errno.ENOENT)
                        if error or errcode and errcode != errno.ENOENT:
                            self.ui.warn(_("trouble committing %s!\n") % f)
                        raise

                # update manifest
                self.ui.note(_("committing manifest\n"))
                removed = [f for f in sorted(removed) if f in m1 or f in m2]
                drop = [f for f in removed if f in m]
                for f in drop:
                    del m[f]
                mn = self.manifest.add(m, trp, linkrev,
                                       p1.manifestnode(), p2.manifestnode(),
                                       added, drop)
                files = changed + removed
            else:
                mn = p1.manifestnode()
                files = []

            # update changelog
            self.ui.note(_("committing changelog\n"))
            self.changelog.delayupdate(tr)
            n = self.changelog.add(mn, files, ctx.description(),
                                   trp, p1.node(), p2.node(),
                                   user, ctx.date(), ctx.extra().copy())
            p = lambda: tr.writepending() and self.root or ""
            xp1, xp2 = p1.hex(), p2 and p2.hex() or ''
            self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                      parent2=xp2, pending=p)
            # set the new commit is proper phase
            targetphase = subrepo.newcommitphase(self.ui, ctx)
            if targetphase:
                # retract boundary do not alter parent changeset.
                # if a parent have higher the resulting phase will
                # be compliant anyway
                #
                # if minimal phase was 0 we don't need to retract anything
                phases.retractboundary(self, tr, targetphase, [n])
            tr.close()
            branchmap.updatecache(self.filtered('served'))
            return n
        finally:
            if tr:
                tr.release()
            lock.release()

    @unfilteredmethod
    def destroying(self):
        '''Inform the repository that nodes are about to be destroyed.
        Intended for use by strip and rollback, so there's a common
        place for anything that has to be done before destroying history.

        This is mostly useful for saving state that is in memory and waiting
        to be flushed when the current lock is released. Because a call to
        destroyed is imminent, the repo will be invalidated causing those
        changes to stay in memory (waiting for the next unlock), or vanish
        completely.
        '''
        # When using the same lock to commit and strip, the phasecache is left
        # dirty after committing. Then when we strip, the repo is invalidated,
        # causing those changes to disappear.
        if '_phasecache' in vars(self):
            self._phasecache.write()

    @unfilteredmethod
    def destroyed(self):
        '''Inform the repository that nodes have been destroyed.
        Intended for use by strip and rollback, so there's a common
        place for anything that has to be done after destroying history.
        '''
        # When one tries to:
        # 1) destroy nodes thus calling this method (e.g. strip)
        # 2) use phasecache somewhere (e.g. commit)
        #
        # then 2) will fail because the phasecache contains nodes that were
        # removed. We can either remove phasecache from the filecache,
        # causing it to reload next time it is accessed, or simply filter
        # the removed nodes now and write the updated cache.
        self._phasecache.filterunknown(self)
        self._phasecache.write()

        # update the 'served' branch cache to help read only server process
        # Thanks to branchcache collaboration this is done from the nearest
        # filtered subset and it is expected to be fast.
        branchmap.updatecache(self.filtered('served'))

        # Ensure the persistent tag cache is updated.  Doing it now
        # means that the tag cache only has to worry about destroyed
        # heads immediately after a strip/rollback.  That in turn
        # guarantees that "cachetip == currenttip" (comparing both rev
        # and node) always means no nodes have been added or destroyed.

        # XXX this is suboptimal when qrefresh'ing: we strip the current
        # head, refresh the tag cache, then immediately add a new head.
        # But I think doing it this way is necessary for the "instant
        # tag cache retrieval" case to work.
        self.invalidate()

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
        '''a convenience method that calls node1.status(node2)'''
        return self[node1].status(node2, match, ignored, clean, unknown,
                                  listsubrepos)

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
        bheads = list(reversed(branches.branchheads(branch, closed=closed)))
        if start is not None:
            # filter out the heads that cannot be reached from startrev
            fbheads = set(self.changelog.nodesbetween([start], bheads)[2])
            bheads = [h for h in bheads if h in fbheads]
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

    def checkpush(self, pushop):
        """Extensions can override this function if additional checks have
        to be performed before pushing, or call it if they override push
        command.
        """
        pass

    @unfilteredpropertycache
    def prepushoutgoinghooks(self):
        """Return util.hooks consists of "(repo, remote, outgoing)"
        functions, which are called before pushing changesets.
        """
        return util.hooks()

    def stream_in(self, remote, remotereqs):
        # Save remote branchmap. We will use it later
        # to speed up branchcache creation
        rbranchmap = None
        if remote.capable("branchmap"):
            rbranchmap = remote.branchmap()

        fp = remote.stream_out()
        l = fp.readline()
        try:
            resp = int(l)
        except ValueError:
            raise error.ResponseError(
                _('unexpected response from remote server:'), l)
        if resp == 1:
            raise util.Abort(_('operation forbidden by server'))
        elif resp == 2:
            raise util.Abort(_('locking the remote repository failed'))
        elif resp != 0:
            raise util.Abort(_('the server sent an unknown error code'))

        self.applystreamclone(remotereqs, rbranchmap, fp)
        return len(self.heads()) + 1

    def applystreamclone(self, remotereqs, remotebranchmap, fp):
        """Apply stream clone data to this repository.

        "remotereqs" is a set of requirements to handle the incoming data.
        "remotebranchmap" is the result of a branchmap lookup on the remote. It
        can be None.
        "fp" is a file object containing the raw stream data, suitable for
        feeding into exchange.consumestreamclone.
        """
        lock = self.lock()
        try:
            exchange.consumestreamclone(self, fp)

            # new requirements = old non-format requirements +
            #                    new format-related remote requirements
            # requirements from the streamed-in repository
            self.requirements = remotereqs | (
                    self.requirements - self.supportedformats)
            self._applyopenerreqs()
            self._writerequirements()

            if remotebranchmap:
                rbheads = []
                closed = []
                for bheads in remotebranchmap.itervalues():
                    rbheads.extend(bheads)
                    for h in bheads:
                        r = self.changelog.rev(h)
                        b, c = self.changelog.branchinfo(r)
                        if c:
                            closed.append(h)

                if rbheads:
                    rtiprev = max((int(self.changelog.rev(node))
                            for node in rbheads))
                    cache = branchmap.branchcache(remotebranchmap,
                                                  self[rtiprev].node(),
                                                  rtiprev,
                                                  closednodes=closed)
                    # Try to stick it as low as possible
                    # filter above served are unlikely to be fetch from a clone
                    for candidate in ('base', 'immutable', 'served'):
                        rview = self.filtered(candidate)
                        if cache.validfor(rview):
                            self._branchcaches[candidate] = cache
                            cache.write(rview)
                            break
            self.invalidate()
        finally:
            lock.release()

    def clone(self, remote, heads=[], stream=None):
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

        if stream is None:
            # if the server explicitly prefers to stream (for fast LANs)
            stream = remote.capable('stream-preferred')

        if stream and not heads:
            # 'stream' means remote revlog format is revlogv1 only
            if remote.capable('stream'):
                self.stream_in(remote, set(('revlogv1',)))
            else:
                # otherwise, 'streamreqs' contains the remote revlog format
                streamreqs = remote.capable('streamreqs')
                if streamreqs:
                    streamreqs = set(streamreqs.split(','))
                    # if we support it, stream in and adjust our requirements
                    if not streamreqs - self.supportedformats:
                        self.stream_in(remote, streamreqs)

        quiet = self.ui.backupconfig('ui', 'quietbookmarkmove')
        try:
            self.ui.setconfig('ui', 'quietbookmarkmove', True, 'clone')
            ret = exchange.pull(self, remote, heads).cgresult
        finally:
            self.ui.restoreconfig(quiet)
        return ret

    def pushkey(self, namespace, key, old, new):
        try:
            tr = self.currenttransaction()
            hookargs = {}
            if tr is not None:
                hookargs.update(tr.hookargs)
                pending = lambda: tr.writepending() and self.root or ""
                hookargs['pending'] = pending
            hookargs['namespace'] = namespace
            hookargs['key'] = key
            hookargs['old'] = old
            hookargs['new'] = new
            self.hook('prepushkey', throw=True, **hookargs)
        except error.HookAbort as exc:
            self.ui.write_err(_("pushkey-abort: %s\n") % exc)
            if exc.hint:
                self.ui.write_err(_("(%s)\n") % exc.hint)
            return False
        self.ui.debug('pushing key for "%s:%s"\n' % (namespace, key))
        ret = pushkey.push(self, namespace, key, old, new)
        def runhook():
            self.hook('pushkey', namespace=namespace, key=key, old=old, new=new,
                      ret=ret)
        self._afterlock(runhook)
        return ret

    def listkeys(self, namespace):
        self.hook('prelistkeys', throw=True, namespace=namespace)
        self.ui.debug('listing keys for "%s"\n' % namespace)
        values = pushkey.list(self, namespace)
        self.hook('listkeys', namespace=namespace, values=values)
        return values

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        '''used to test argument passing over the wire'''
        return "%s %s %s %s %s" % (one, two, three, four, five)

    def savecommitmessage(self, text):
        fp = self.vfs('last-message.txt', 'wb')
        try:
            fp.write(text)
        finally:
            fp.close()
        return self.pathto(fp.name[len(self.root) + 1:])

# used to avoid circular references so destructors work
def aftertrans(files):
    renamefiles = [tuple(t) for t in files]
    def a():
        for vfs, src, dest in renamefiles:
            try:
                vfs.rename(src, dest)
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
