# context.py - changeset and file context objects for mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid, nullrev, wdirid, short, hex, bin
from i18n import _
import mdiff, error, util, scmutil, subrepo, patch, encoding, phases
import match as matchmod
import os, errno, stat
import obsolete as obsmod
import repoview
import fileset
import revlog

propertycache = util.propertycache

# Phony node value to stand-in for new files in some uses of
# manifests. Manifests support 21-byte hashes for nodes which are
# dirty in the working copy.
_newnode = '!' * 21

class basectx(object):
    """A basectx object represents the common logic for its children:
    changectx: read-only context that is already present in the repo,
    workingctx: a context that represents the working directory and can
                be committed,
    memctx: a context that represents changes in-memory and can also
            be committed."""
    def __new__(cls, repo, changeid='', *args, **kwargs):
        if isinstance(changeid, basectx):
            return changeid

        o = super(basectx, cls).__new__(cls)

        o._repo = repo
        o._rev = nullrev
        o._node = nullid

        return o

    def __str__(self):
        return short(self.node())

    def __int__(self):
        return self.rev()

    def __repr__(self):
        return "<%s %s>" % (type(self).__name__, str(self))

    def __eq__(self, other):
        try:
            return type(self) == type(other) and self._rev == other._rev
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def __contains__(self, key):
        return key in self._manifest

    def __getitem__(self, key):
        return self.filectx(key)

    def __iter__(self):
        return iter(self._manifest)

    def _manifestmatches(self, match, s):
        """generate a new manifest filtered by the match argument

        This method is for internal use only and mainly exists to provide an
        object oriented way for other contexts to customize the manifest
        generation.
        """
        return self.manifest().matches(match)

    def _matchstatus(self, other, match):
        """return match.always if match is none

        This internal method provides a way for child objects to override the
        match operator.
        """
        return match or matchmod.always(self._repo.root, self._repo.getcwd())

    def _buildstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """build a status with respect to another context"""
        # Load earliest manifest first for caching reasons. More specifically,
        # if you have revisions 1000 and 1001, 1001 is probably stored as a
        # delta against 1000. Thus, if you read 1000 first, we'll reconstruct
        # 1000 and cache it so that when you read 1001, we just need to apply a
        # delta to what's in the cache. So that's one full reconstruction + one
        # delta application.
        if self.rev() is not None and self.rev() < other.rev():
            self.manifest()
        mf1 = other._manifestmatches(match, s)
        mf2 = self._manifestmatches(match, s)

        modified, added = [], []
        removed = []
        clean = []
        deleted, unknown, ignored = s.deleted, s.unknown, s.ignored
        deletedset = set(deleted)
        d = mf1.diff(mf2, clean=listclean)
        for fn, value in d.iteritems():
            if fn in deletedset:
                continue
            if value is None:
                clean.append(fn)
                continue
            (node1, flag1), (node2, flag2) = value
            if node1 is None:
                added.append(fn)
            elif node2 is None:
                removed.append(fn)
            elif node2 != _newnode:
                # The file was not a new file in mf2, so an entry
                # from diff is really a difference.
                modified.append(fn)
            elif self[fn].cmp(other[fn]):
                # node2 was newnode, but the working file doesn't
                # match the one in mf1.
                modified.append(fn)
            else:
                clean.append(fn)

        if removed:
            # need to filter files if they are already reported as removed
            unknown = [fn for fn in unknown if fn not in mf1]
            ignored = [fn for fn in ignored if fn not in mf1]
            # if they're deleted, don't report them as removed
            removed = [fn for fn in removed if fn not in deletedset]

        return scmutil.status(modified, added, removed, deleted, unknown,
                              ignored, clean)

    @propertycache
    def substate(self):
        return subrepo.state(self, self._repo.ui)

    def subrev(self, subpath):
        return self.substate[subpath][1]

    def rev(self):
        return self._rev
    def node(self):
        return self._node
    def hex(self):
        return hex(self.node())
    def manifest(self):
        return self._manifest
    def repo(self):
        return self._repo
    def phasestr(self):
        return phases.phasenames[self.phase()]
    def mutable(self):
        return self.phase() > phases.public

    def getfileset(self, expr):
        return fileset.getfileset(self, expr)

    def obsolete(self):
        """True if the changeset is obsolete"""
        return self.rev() in obsmod.getrevs(self._repo, 'obsolete')

    def extinct(self):
        """True if the changeset is extinct"""
        return self.rev() in obsmod.getrevs(self._repo, 'extinct')

    def unstable(self):
        """True if the changeset is not obsolete but it's ancestor are"""
        return self.rev() in obsmod.getrevs(self._repo, 'unstable')

    def bumped(self):
        """True if the changeset try to be a successor of a public changeset

        Only non-public and non-obsolete changesets may be bumped.
        """
        return self.rev() in obsmod.getrevs(self._repo, 'bumped')

    def divergent(self):
        """Is a successors of a changeset with multiple possible successors set

        Only non-public and non-obsolete changesets may be divergent.
        """
        return self.rev() in obsmod.getrevs(self._repo, 'divergent')

    def troubled(self):
        """True if the changeset is either unstable, bumped or divergent"""
        return self.unstable() or self.bumped() or self.divergent()

    def troubles(self):
        """return the list of troubles affecting this changesets.

        Troubles are returned as strings. possible values are:
        - unstable,
        - bumped,
        - divergent.
        """
        troubles = []
        if self.unstable():
            troubles.append('unstable')
        if self.bumped():
            troubles.append('bumped')
        if self.divergent():
            troubles.append('divergent')
        return troubles

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def p1(self):
        return self._parents[0]

    def p2(self):
        if len(self._parents) == 2:
            return self._parents[1]
        return changectx(self._repo, -1)

    def _fileinfo(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest[path], self._manifest.flags(path)
            except KeyError:
                raise error.ManifestLookupError(self._node, path,
                                                _('not found in manifest'))
        if '_manifestdelta' in self.__dict__ or path in self.files():
            if path in self._manifestdelta:
                return (self._manifestdelta[path],
                        self._manifestdelta.flags(path))
        node, flag = self._repo.manifest.find(self._changeset[0], path)
        if not node:
            raise error.ManifestLookupError(self._node, path,
                                            _('not found in manifest'))

        return node, flag

    def filenode(self, path):
        return self._fileinfo(path)[0]

    def flags(self, path):
        try:
            return self._fileinfo(path)[1]
        except error.LookupError:
            return ''

    def sub(self, path):
        '''return a subrepo for the stored revision of path, never wdir()'''
        return subrepo.subrepo(self, path)

    def nullsub(self, path, pctx):
        return subrepo.nullsubrepo(self, path, pctx)

    def workingsub(self, path):
        '''return a subrepo for the stored revision, or wdir if this is a wdir
        context.
        '''
        return subrepo.subrepo(self, path, allowwdir=True)

    def match(self, pats=[], include=None, exclude=None, default='glob',
              listsubrepos=False, badfn=None):
        r = self._repo
        return matchmod.match(r.root, r.getcwd(), pats,
                              include, exclude, default,
                              auditor=r.auditor, ctx=self,
                              listsubrepos=listsubrepos, badfn=badfn)

    def diff(self, ctx2=None, match=None, **opts):
        """Returns a diff generator for the given contexts and matcher"""
        if ctx2 is None:
            ctx2 = self.p1()
        if ctx2 is not None:
            ctx2 = self._repo[ctx2]
        diffopts = patch.diffopts(self._repo.ui, opts)
        return patch.diff(self._repo, ctx2, self, match=match, opts=diffopts)

    def dirs(self):
        return self._manifest.dirs()

    def hasdir(self, dir):
        return self._manifest.hasdir(dir)

    def dirty(self, missing=False, merge=True, branch=True):
        return False

    def status(self, other=None, match=None, listignored=False,
               listclean=False, listunknown=False, listsubrepos=False):
        """return status of files between two nodes or node and working
        directory.

        If other is None, compare this node with working directory.

        returns (modified, added, removed, deleted, unknown, ignored, clean)
        """

        ctx1 = self
        ctx2 = self._repo[other]

        # This next code block is, admittedly, fragile logic that tests for
        # reversing the contexts and wouldn't need to exist if it weren't for
        # the fast (and common) code path of comparing the working directory
        # with its first parent.
        #
        # What we're aiming for here is the ability to call:
        #
        # workingctx.status(parentctx)
        #
        # If we always built the manifest for each context and compared those,
        # then we'd be done. But the special case of the above call means we
        # just copy the manifest of the parent.
        reversed = False
        if (not isinstance(ctx1, changectx)
            and isinstance(ctx2, changectx)):
            reversed = True
            ctx1, ctx2 = ctx2, ctx1

        match = ctx2._matchstatus(ctx1, match)
        r = scmutil.status([], [], [], [], [], [], [])
        r = ctx2._buildstatus(ctx1, r, match, listignored, listclean,
                              listunknown)

        if reversed:
            # Reverse added and removed. Clear deleted, unknown and ignored as
            # these make no sense to reverse.
            r = scmutil.status(r.modified, r.removed, r.added, [], [], [],
                               r.clean)

        if listsubrepos:
            for subpath, sub in scmutil.itersubrepos(ctx1, ctx2):
                rev2 = ctx2.subrev(subpath)
                try:
                    submatch = matchmod.narrowmatcher(subpath, match)
                    s = sub.status(rev2, match=submatch, ignored=listignored,
                                   clean=listclean, unknown=listunknown,
                                   listsubrepos=True)
                    for rfiles, sfiles in zip(r, s):
                        rfiles.extend("%s/%s" % (subpath, f) for f in sfiles)
                except error.LookupError:
                    self._repo.ui.status(_("skipping missing "
                                           "subrepository: %s\n") % subpath)

        for l in r:
            l.sort()

        return r


def makememctx(repo, parents, text, user, date, branch, files, store,
               editor=None, extra=None):
    def getfilectx(repo, memctx, path):
        data, mode, copied = store.getfile(path)
        if data is None:
            return None
        islink, isexec = mode
        return memfilectx(repo, path, data, islink=islink, isexec=isexec,
                                  copied=copied, memctx=memctx)
    if extra is None:
        extra = {}
    if branch:
        extra['branch'] = encoding.fromlocal(branch)
    ctx =  memctx(repo, parents, text, files, getfilectx, user,
                          date, extra, editor)
    return ctx

class changectx(basectx):
    """A changecontext object makes access to data related to a particular
    changeset convenient. It represents a read-only context already present in
    the repo."""
    def __init__(self, repo, changeid=''):
        """changeid is a revision number, node, or tag"""

        # since basectx.__new__ already took care of copying the object, we
        # don't need to do anything in __init__, so we just exit here
        if isinstance(changeid, basectx):
            return

        if changeid == '':
            changeid = '.'
        self._repo = repo

        try:
            if isinstance(changeid, int):
                self._node = repo.changelog.node(changeid)
                self._rev = changeid
                return
            if isinstance(changeid, long):
                changeid = str(changeid)
            if changeid == 'null':
                self._node = nullid
                self._rev = nullrev
                return
            if changeid == 'tip':
                self._node = repo.changelog.tip()
                self._rev = repo.changelog.rev(self._node)
                return
            if changeid == '.' or changeid == repo.dirstate.p1():
                # this is a hack to delay/avoid loading obsmarkers
                # when we know that '.' won't be hidden
                self._node = repo.dirstate.p1()
                self._rev = repo.unfiltered().changelog.rev(self._node)
                return
            if len(changeid) == 20:
                try:
                    self._node = changeid
                    self._rev = repo.changelog.rev(changeid)
                    return
                except error.FilteredRepoLookupError:
                    raise
                except LookupError:
                    pass

            try:
                r = int(changeid)
                if str(r) != changeid:
                    raise ValueError
                l = len(repo.changelog)
                if r < 0:
                    r += l
                if r < 0 or r >= l:
                    raise ValueError
                self._rev = r
                self._node = repo.changelog.node(r)
                return
            except error.FilteredIndexError:
                raise
            except (ValueError, OverflowError, IndexError):
                pass

            if len(changeid) == 40:
                try:
                    self._node = bin(changeid)
                    self._rev = repo.changelog.rev(self._node)
                    return
                except error.FilteredLookupError:
                    raise
                except (TypeError, LookupError):
                    pass

            # lookup bookmarks through the name interface
            try:
                self._node = repo.names.singlenode(repo, changeid)
                self._rev = repo.changelog.rev(self._node)
                return
            except KeyError:
                pass
            except error.FilteredRepoLookupError:
                raise
            except error.RepoLookupError:
                pass

            self._node = repo.unfiltered().changelog._partialmatch(changeid)
            if self._node is not None:
                self._rev = repo.changelog.rev(self._node)
                return

            # lookup failed
            # check if it might have come from damaged dirstate
            #
            # XXX we could avoid the unfiltered if we had a recognizable
            # exception for filtered changeset access
            if changeid in repo.unfiltered().dirstate.parents():
                msg = _("working directory has unknown parent '%s'!")
                raise error.Abort(msg % short(changeid))
            try:
                if len(changeid) == 20:
                    changeid = hex(changeid)
            except TypeError:
                pass
        except (error.FilteredIndexError, error.FilteredLookupError,
                error.FilteredRepoLookupError):
            if repo.filtername.startswith('visible'):
                msg = _("hidden revision '%s'") % changeid
                hint = _('use --hidden to access hidden revisions')
                raise error.FilteredRepoLookupError(msg, hint=hint)
            msg = _("filtered revision '%s' (not in '%s' subset)")
            msg %= (changeid, repo.filtername)
            raise error.FilteredRepoLookupError(msg)
        except IndexError:
            pass
        raise error.RepoLookupError(
            _("unknown revision '%s'") % changeid)

    def __hash__(self):
        try:
            return hash(self._rev)
        except AttributeError:
            return id(self)

    def __nonzero__(self):
        return self._rev != nullrev

    @propertycache
    def _changeset(self):
        return self._repo.changelog.read(self.rev())

    @propertycache
    def _manifest(self):
        return self._repo.manifest.read(self._changeset[0])

    @propertycache
    def _manifestdelta(self):
        return self._repo.manifest.readdelta(self._changeset[0])

    @propertycache
    def _parents(self):
        p = self._repo.changelog.parentrevs(self._rev)
        if p[1] == nullrev:
            p = p[:-1]
        return [changectx(self._repo, x) for x in p]

    def changeset(self):
        return self._changeset
    def manifestnode(self):
        return self._changeset[0]

    def user(self):
        return self._changeset[1]
    def date(self):
        return self._changeset[2]
    def files(self):
        return self._changeset[3]
    def description(self):
        return self._changeset[4]
    def branch(self):
        return encoding.tolocal(self._changeset[5].get("branch"))
    def closesbranch(self):
        return 'close' in self._changeset[5]
    def extra(self):
        return self._changeset[5]
    def tags(self):
        return self._repo.nodetags(self._node)
    def bookmarks(self):
        return self._repo.nodebookmarks(self._node)
    def phase(self):
        return self._repo._phasecache.phase(self._repo, self._rev)
    def hidden(self):
        return self._rev in repoview.filterrevs(self._repo, 'visible')

    def children(self):
        """return contexts for each child changeset"""
        c = self._repo.changelog.children(self._node)
        return [changectx(self._repo, x) for x in c]

    def ancestors(self):
        for a in self._repo.changelog.ancestors([self._rev]):
            yield changectx(self._repo, a)

    def descendants(self):
        for d in self._repo.changelog.descendants([self._rev]):
            yield changectx(self._repo, d)

    def filectx(self, path, fileid=None, filelog=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid,
                       changectx=self, filelog=filelog)

    def ancestor(self, c2, warn=False):
        """return the "best" ancestor context of self and c2

        If there are multiple candidates, it will show a message and check
        merge.preferancestor configuration before falling back to the
        revlog ancestor."""
        # deal with workingctxs
        n2 = c2._node
        if n2 is None:
            n2 = c2._parents[0]._node
        cahs = self._repo.changelog.commonancestorsheads(self._node, n2)
        if not cahs:
            anc = nullid
        elif len(cahs) == 1:
            anc = cahs[0]
        else:
            # experimental config: merge.preferancestor
            for r in self._repo.ui.configlist('merge', 'preferancestor', ['*']):
                try:
                    ctx = changectx(self._repo, r)
                except error.RepoLookupError:
                    continue
                anc = ctx.node()
                if anc in cahs:
                    break
            else:
                anc = self._repo.changelog.ancestor(self._node, n2)
            if warn:
                self._repo.ui.status(
                    (_("note: using %s as ancestor of %s and %s\n") %
                     (short(anc), short(self._node), short(n2))) +
                    ''.join(_("      alternatively, use --config "
                              "merge.preferancestor=%s\n") %
                            short(n) for n in sorted(cahs) if n != anc))
        return changectx(self._repo, anc)

    def descendant(self, other):
        """True if other is descendant of this changeset"""
        return self._repo.changelog.descendant(self._rev, other._rev)

    def walk(self, match):
        '''Generates matching file names.'''

        # Wrap match.bad method to have message with nodeid
        def bad(fn, msg):
            # The manifest doesn't know about subrepos, so don't complain about
            # paths into valid subrepos.
            if any(fn == s or fn.startswith(s + '/')
                   for s in self.substate):
                return
            match.bad(fn, _('no such file in rev %s') % self)

        m = matchmod.badmatch(match, bad)
        return self._manifest.walk(m)

    def matches(self, match):
        return self.walk(match)

class basefilectx(object):
    """A filecontext object represents the common logic for its children:
    filectx: read-only access to a filerevision that is already present
             in the repo,
    workingfilectx: a filecontext that represents files from the working
                    directory,
    memfilectx: a filecontext that represents files in-memory."""
    def __new__(cls, repo, path, *args, **kwargs):
        return super(basefilectx, cls).__new__(cls)

    @propertycache
    def _filelog(self):
        return self._repo.file(self._path)

    @propertycache
    def _changeid(self):
        if '_changeid' in self.__dict__:
            return self._changeid
        elif '_changectx' in self.__dict__:
            return self._changectx.rev()
        elif '_descendantrev' in self.__dict__:
            # this file context was created from a revision with a known
            # descendant, we can (lazily) correct for linkrev aliases
            return self._adjustlinkrev(self._path, self._filelog,
                                       self._filenode, self._descendantrev)
        else:
            return self._filelog.linkrev(self._filerev)

    @propertycache
    def _filenode(self):
        if '_fileid' in self.__dict__:
            return self._filelog.lookup(self._fileid)
        else:
            return self._changectx.filenode(self._path)

    @propertycache
    def _filerev(self):
        return self._filelog.rev(self._filenode)

    @propertycache
    def _repopath(self):
        return self._path

    def __nonzero__(self):
        try:
            self._filenode
            return True
        except error.LookupError:
            # file is missing
            return False

    def __str__(self):
        return "%s@%s" % (self.path(), self._changectx)

    def __repr__(self):
        return "<%s %s>" % (type(self).__name__, str(self))

    def __hash__(self):
        try:
            return hash((self._path, self._filenode))
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            return (type(self) == type(other) and self._path == other._path
                    and self._filenode == other._filenode)
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def filerev(self):
        return self._filerev
    def filenode(self):
        return self._filenode
    def flags(self):
        return self._changectx.flags(self._path)
    def filelog(self):
        return self._filelog
    def rev(self):
        return self._changeid
    def linkrev(self):
        return self._filelog.linkrev(self._filerev)
    def node(self):
        return self._changectx.node()
    def hex(self):
        return self._changectx.hex()
    def user(self):
        return self._changectx.user()
    def date(self):
        return self._changectx.date()
    def files(self):
        return self._changectx.files()
    def description(self):
        return self._changectx.description()
    def branch(self):
        return self._changectx.branch()
    def extra(self):
        return self._changectx.extra()
    def phase(self):
        return self._changectx.phase()
    def phasestr(self):
        return self._changectx.phasestr()
    def manifest(self):
        return self._changectx.manifest()
    def changectx(self):
        return self._changectx
    def repo(self):
        return self._repo

    def path(self):
        return self._path

    def isbinary(self):
        try:
            return util.binary(self.data())
        except IOError:
            return False
    def isexec(self):
        return 'x' in self.flags()
    def islink(self):
        return 'l' in self.flags()

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        if (fctx._filerev is None
            and (self._repo._encodefilterpats
                 # if file data starts with '\1\n', empty metadata block is
                 # prepended, which adds 4 bytes to filelog.size().
                 or self.size() - 4 == fctx.size())
            or self.size() == fctx.size()):
            return self._filelog.cmp(self._filenode, fctx.data())

        return True

    def _adjustlinkrev(self, path, filelog, fnode, srcrev, inclusive=False):
        """return the first ancestor of <srcrev> introducing <fnode>

        If the linkrev of the file revision does not point to an ancestor of
        srcrev, we'll walk down the ancestors until we find one introducing
        this file revision.

        :repo: a localrepository object (used to access changelog and manifest)
        :path: the file path
        :fnode: the nodeid of the file revision
        :filelog: the filelog of this path
        :srcrev: the changeset revision we search ancestors from
        :inclusive: if true, the src revision will also be checked
        """
        repo = self._repo
        cl = repo.unfiltered().changelog
        ma = repo.manifest
        # fetch the linkrev
        fr = filelog.rev(fnode)
        lkr = filelog.linkrev(fr)
        # hack to reuse ancestor computation when searching for renames
        memberanc = getattr(self, '_ancestrycontext', None)
        iteranc = None
        if srcrev is None:
            # wctx case, used by workingfilectx during mergecopy
            revs = [p.rev() for p in self._repo[None].parents()]
            inclusive = True # we skipped the real (revless) source
        else:
            revs = [srcrev]
        if memberanc is None:
            memberanc = iteranc = cl.ancestors(revs, lkr,
                                               inclusive=inclusive)
        # check if this linkrev is an ancestor of srcrev
        if lkr not in memberanc:
            if iteranc is None:
                iteranc = cl.ancestors(revs, lkr, inclusive=inclusive)
            for a in iteranc:
                ac = cl.read(a) # get changeset data (we avoid object creation)
                if path in ac[3]: # checking the 'files' field.
                    # The file has been touched, check if the content is
                    # similar to the one we search for.
                    if fnode == ma.readfast(ac[0]).get(path):
                        return a
            # In theory, we should never get out of that loop without a result.
            # But if manifest uses a buggy file revision (not children of the
            # one it replaces) we could. Such a buggy situation will likely
            # result is crash somewhere else at to some point.
        return lkr

    def introrev(self):
        """return the rev of the changeset which introduced this file revision

        This method is different from linkrev because it take into account the
        changeset the filectx was created from. It ensures the returned
        revision is one of its ancestors. This prevents bugs from
        'linkrev-shadowing' when a file revision is used by multiple
        changesets.
        """
        lkr = self.linkrev()
        attrs = vars(self)
        noctx = not ('_changeid' in attrs or '_changectx' in attrs)
        if noctx or self.rev() == lkr:
            return self.linkrev()
        return self._adjustlinkrev(self._path, self._filelog, self._filenode,
                                   self.rev(), inclusive=True)

    def _parentfilectx(self, path, fileid, filelog):
        """create parent filectx keeping ancestry info for _adjustlinkrev()"""
        fctx = filectx(self._repo, path, fileid=fileid, filelog=filelog)
        if '_changeid' in vars(self) or '_changectx' in vars(self):
            # If self is associated with a changeset (probably explicitly
            # fed), ensure the created filectx is associated with a
            # changeset that is an ancestor of self.changectx.
            # This lets us later use _adjustlinkrev to get a correct link.
            fctx._descendantrev = self.rev()
            fctx._ancestrycontext = getattr(self, '_ancestrycontext', None)
        elif '_descendantrev' in vars(self):
            # Otherwise propagate _descendantrev if we have one associated.
            fctx._descendantrev = self._descendantrev
            fctx._ancestrycontext = getattr(self, '_ancestrycontext', None)
        return fctx

    def parents(self):
        _path = self._path
        fl = self._filelog
        parents = self._filelog.parents(self._filenode)
        pl = [(_path, node, fl) for node in parents if node != nullid]

        r = fl.renamed(self._filenode)
        if r:
            # - In the simple rename case, both parent are nullid, pl is empty.
            # - In case of merge, only one of the parent is null id and should
            # be replaced with the rename information. This parent is -always-
            # the first one.
            #
            # As null id have always been filtered out in the previous list
            # comprehension, inserting to 0 will always result in "replacing
            # first nullid parent with rename information.
            pl.insert(0, (r[0], r[1], self._repo.file(r[0])))

        return [self._parentfilectx(path, fnode, l) for path, fnode, l in pl]

    def p1(self):
        return self.parents()[0]

    def p2(self):
        p = self.parents()
        if len(p) == 2:
            return p[1]
        return filectx(self._repo, self._path, fileid=-1, filelog=self._filelog)

    def annotate(self, follow=False, linenumber=None, diffopts=None):
        '''returns a list of tuples of (ctx, line) for each line
        in the file, where ctx is the filectx of the node where
        that line was last changed.
        This returns tuples of ((ctx, linenumber), line) for each line,
        if "linenumber" parameter is NOT "None".
        In such tuples, linenumber means one at the first appearance
        in the managed file.
        To reduce annotation cost,
        this returns fixed value(False is used) as linenumber,
        if "linenumber" parameter is "False".'''

        if linenumber is None:
            def decorate(text, rev):
                return ([rev] * len(text.splitlines()), text)
        elif linenumber:
            def decorate(text, rev):
                size = len(text.splitlines())
                return ([(rev, i) for i in xrange(1, size + 1)], text)
        else:
            def decorate(text, rev):
                return ([(rev, False)] * len(text.splitlines()), text)

        def pair(parent, child):
            blocks = mdiff.allblocks(parent[1], child[1], opts=diffopts,
                                     refine=True)
            for (a1, a2, b1, b2), t in blocks:
                # Changed blocks ('!') or blocks made only of blank lines ('~')
                # belong to the child.
                if t == '=':
                    child[0][b1:b2] = parent[0][a1:a2]
            return child

        getlog = util.lrucachefunc(lambda x: self._repo.file(x))

        def parents(f):
            # Cut _descendantrev here to mitigate the penalty of lazy linkrev
            # adjustment. Otherwise, p._adjustlinkrev() would walk changelog
            # from the topmost introrev (= srcrev) down to p.linkrev() if it
            # isn't an ancestor of the srcrev.
            f._changeid
            pl = f.parents()

            # Don't return renamed parents if we aren't following.
            if not follow:
                pl = [p for p in pl if p.path() == f.path()]

            # renamed filectx won't have a filelog yet, so set it
            # from the cache to save time
            for p in pl:
                if not '_filelog' in p.__dict__:
                    p._filelog = getlog(p.path())

            return pl

        # use linkrev to find the first changeset where self appeared
        base = self
        introrev = self.introrev()
        if self.rev() != introrev:
            base = self.filectx(self.filenode(), changeid=introrev)
        if getattr(base, '_ancestrycontext', None) is None:
            cl = self._repo.changelog
            if introrev is None:
                # wctx is not inclusive, but works because _ancestrycontext
                # is used to test filelog revisions
                ac = cl.ancestors([p.rev() for p in base.parents()],
                                  inclusive=True)
            else:
                ac = cl.ancestors([introrev], inclusive=True)
            base._ancestrycontext = ac

        # This algorithm would prefer to be recursive, but Python is a
        # bit recursion-hostile. Instead we do an iterative
        # depth-first search.

        visit = [base]
        hist = {}
        pcache = {}
        needed = {base: 1}
        while visit:
            f = visit[-1]
            pcached = f in pcache
            if not pcached:
                pcache[f] = parents(f)

            ready = True
            pl = pcache[f]
            for p in pl:
                if p not in hist:
                    ready = False
                    visit.append(p)
                if not pcached:
                    needed[p] = needed.get(p, 0) + 1
            if ready:
                visit.pop()
                reusable = f in hist
                if reusable:
                    curr = hist[f]
                else:
                    curr = decorate(f.data(), f)
                for p in pl:
                    if not reusable:
                        curr = pair(hist[p], curr)
                    if needed[p] == 1:
                        del hist[p]
                        del needed[p]
                    else:
                        needed[p] -= 1

                hist[f] = curr
                pcache[f] = []

        return zip(hist[base][0], hist[base][1].splitlines(True))

    def ancestors(self, followfirst=False):
        visit = {}
        c = self
        if followfirst:
            cut = 1
        else:
            cut = None

        while True:
            for parent in c.parents()[:cut]:
                visit[(parent.linkrev(), parent.filenode())] = parent
            if not visit:
                break
            c = visit.pop(max(visit))
            yield c

class filectx(basefilectx):
    """A filecontext object makes access to data related to a particular
       filerevision convenient."""
    def __init__(self, repo, path, changeid=None, fileid=None,
                 filelog=None, changectx=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        self._repo = repo
        self._path = path

        assert (changeid is not None
                or fileid is not None
                or changectx is not None), \
                ("bad args: changeid=%r, fileid=%r, changectx=%r"
                 % (changeid, fileid, changectx))

        if filelog is not None:
            self._filelog = filelog

        if changeid is not None:
            self._changeid = changeid
        if changectx is not None:
            self._changectx = changectx
        if fileid is not None:
            self._fileid = fileid

    @propertycache
    def _changectx(self):
        try:
            return changectx(self._repo, self._changeid)
        except error.FilteredRepoLookupError:
            # Linkrev may point to any revision in the repository.  When the
            # repository is filtered this may lead to `filectx` trying to build
            # `changectx` for filtered revision. In such case we fallback to
            # creating `changectx` on the unfiltered version of the reposition.
            # This fallback should not be an issue because `changectx` from
            # `filectx` are not used in complex operations that care about
            # filtering.
            #
            # This fallback is a cheap and dirty fix that prevent several
            # crashes. It does not ensure the behavior is correct. However the
            # behavior was not correct before filtering either and "incorrect
            # behavior" is seen as better as "crash"
            #
            # Linkrevs have several serious troubles with filtering that are
            # complicated to solve. Proper handling of the issue here should be
            # considered when solving linkrev issue are on the table.
            return changectx(self._repo.unfiltered(), self._changeid)

    def filectx(self, fileid, changeid=None):
        '''opens an arbitrary revision of the file without
        opening a new filelog'''
        return filectx(self._repo, self._path, fileid=fileid,
                       filelog=self._filelog, changeid=changeid)

    def data(self):
        try:
            return self._filelog.read(self._filenode)
        except error.CensoredNodeError:
            if self._repo.ui.config("censor", "policy", "abort") == "ignore":
                return ""
            raise util.Abort(_("censored node: %s") % short(self._filenode),
                             hint=_("set censor.policy to ignore errors"))

    def size(self):
        return self._filelog.size(self._filerev)

    def renamed(self):
        """check if file was actually renamed in this changeset revision

        If rename logged in file revision, we report copy for changeset only
        if file revisions linkrev points back to the changeset in question
        or both changeset parents contain different file revisions.
        """

        renamed = self._filelog.renamed(self._filenode)
        if not renamed:
            return renamed

        if self.rev() == self.linkrev():
            return renamed

        name = self.path()
        fnode = self._filenode
        for p in self._changectx.parents():
            try:
                if fnode == p.filenode(name):
                    return None
            except error.LookupError:
                pass
        return renamed

    def children(self):
        # hard for renames
        c = self._filelog.children(self._filenode)
        return [filectx(self._repo, self._path, fileid=x,
                        filelog=self._filelog) for x in c]

class committablectx(basectx):
    """A committablectx object provides common functionality for a context that
    wants the ability to commit, e.g. workingctx or memctx."""
    def __init__(self, repo, text="", user=None, date=None, extra=None,
                 changes=None):
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        if date:
            self._date = util.parsedate(date)
        if user:
            self._user = user
        if changes:
            self._status = changes

        self._extra = {}
        if extra:
            self._extra = extra.copy()
        if 'branch' not in self._extra:
            try:
                branch = encoding.fromlocal(self._repo.dirstate.branch())
            except UnicodeDecodeError:
                raise util.Abort(_('branch name not in UTF-8!'))
            self._extra['branch'] = branch
        if self._extra['branch'] == '':
            self._extra['branch'] = 'default'

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __nonzero__(self):
        return True

    def _buildflagfunc(self):
        # Create a fallback function for getting file flags when the
        # filesystem doesn't support them

        copiesget = self._repo.dirstate.copies().get

        if len(self._parents) < 2:
            # when we have one parent, it's easy: copy from parent
            man = self._parents[0].manifest()
            def func(f):
                f = copiesget(f, f)
                return man.flags(f)
        else:
            # merges are tricky: we try to reconstruct the unstored
            # result from the merge (issue1802)
            p1, p2 = self._parents
            pa = p1.ancestor(p2)
            m1, m2, ma = p1.manifest(), p2.manifest(), pa.manifest()

            def func(f):
                f = copiesget(f, f) # may be wrong for merges with copies
                fl1, fl2, fla = m1.flags(f), m2.flags(f), ma.flags(f)
                if fl1 == fl2:
                    return fl1
                if fl1 == fla:
                    return fl2
                if fl2 == fla:
                    return fl1
                return '' # punt for conflicts

        return func

    @propertycache
    def _flagfunc(self):
        return self._repo.dirstate.flagfunc(self._buildflagfunc)

    @propertycache
    def _manifest(self):
        """generate a manifest corresponding to the values in self._status

        This reuse the file nodeid from parent, but we append an extra letter
        when modified. Modified files get an extra 'm' while added files get
        an extra 'a'. This is used by manifests merge to see that files
        are different and by update logic to avoid deleting newly added files.
        """

        man1 = self._parents[0].manifest()
        man = man1.copy()
        if len(self._parents) > 1:
            man2 = self.p2().manifest()
            def getman(f):
                if f in man1:
                    return man1
                return man2
        else:
            getman = lambda f: man1

        copied = self._repo.dirstate.copies()
        ff = self._flagfunc
        for i, l in (("a", self._status.added), ("m", self._status.modified)):
            for f in l:
                orig = copied.get(f, f)
                man[f] = getman(orig).get(orig, nullid) + i
                try:
                    man.setflag(f, ff(f))
                except OSError:
                    pass

        for f in self._status.deleted + self._status.removed:
            if f in man:
                del man[f]

        return man

    @propertycache
    def _status(self):
        return self._repo.status()

    @propertycache
    def _user(self):
        return self._repo.ui.username()

    @propertycache
    def _date(self):
        return util.makedate()

    def subrev(self, subpath):
        return None

    def manifestnode(self):
        return None
    def user(self):
        return self._user or self._repo.ui.username()
    def date(self):
        return self._date
    def description(self):
        return self._text
    def files(self):
        return sorted(self._status.modified + self._status.added +
                      self._status.removed)

    def modified(self):
        return self._status.modified
    def added(self):
        return self._status.added
    def removed(self):
        return self._status.removed
    def deleted(self):
        return self._status.deleted
    def branch(self):
        return encoding.tolocal(self._extra['branch'])
    def closesbranch(self):
        return 'close' in self._extra
    def extra(self):
        return self._extra

    def tags(self):
        return []

    def bookmarks(self):
        b = []
        for p in self.parents():
            b.extend(p.bookmarks())
        return b

    def phase(self):
        phase = phases.draft # default phase to draft
        for p in self.parents():
            phase = max(phase, p.phase())
        return phase

    def hidden(self):
        return False

    def children(self):
        return []

    def flags(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest.flags(path)
            except KeyError:
                return ''

        try:
            return self._flagfunc(path)
        except OSError:
            return ''

    def ancestor(self, c2):
        """return the "best" ancestor context of self and c2"""
        return self._parents[0].ancestor(c2) # punt on two parents for now

    def walk(self, match):
        '''Generates matching file names.'''
        return sorted(self._repo.dirstate.walk(match, sorted(self.substate),
                                               True, False))

    def matches(self, match):
        return sorted(self._repo.dirstate.matches(match))

    def ancestors(self):
        for p in self._parents:
            yield p
        for a in self._repo.changelog.ancestors(
            [p.rev() for p in self._parents]):
            yield changectx(self._repo, a)

    def markcommitted(self, node):
        """Perform post-commit cleanup necessary after committing this ctx

        Specifically, this updates backing stores this working context
        wraps to reflect the fact that the changes reflected by this
        workingctx have been committed.  For example, it marks
        modified and added files as normal in the dirstate.

        """

        self._repo.dirstate.beginparentchange()
        for f in self.modified() + self.added():
            self._repo.dirstate.normal(f)
        for f in self.removed():
            self._repo.dirstate.drop(f)
        self._repo.dirstate.setparents(node)
        self._repo.dirstate.endparentchange()

        # write changes out explicitly, because nesting wlock at
        # runtime may prevent 'wlock.release()' in 'repo.commit()'
        # from immediately doing so for subsequent changing files
        self._repo.dirstate.write()

class workingctx(committablectx):
    """A workingctx object makes access to data related to
    the current working directory convenient.
    date - any valid date string or (unixtime, offset), or None.
    user - username string, or None.
    extra - a dictionary of extra values, or None.
    changes - a list of file lists as returned by localrepo.status()
               or None to use the repository status.
    """
    def __init__(self, repo, text="", user=None, date=None, extra=None,
                 changes=None):
        super(workingctx, self).__init__(repo, text, user, date, extra, changes)

    def __iter__(self):
        d = self._repo.dirstate
        for f in d:
            if d[f] != 'r':
                yield f

    def __contains__(self, key):
        return self._repo.dirstate[key] not in "?r"

    def hex(self):
        return hex(wdirid)

    @propertycache
    def _parents(self):
        p = self._repo.dirstate.parents()
        if p[1] == nullid:
            p = p[:-1]
        return [changectx(self._repo, x) for x in p]

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return workingfilectx(self._repo, path, workingctx=self,
                              filelog=filelog)

    def dirty(self, missing=False, merge=True, branch=True):
        "check whether a working directory is modified"
        # check subrepos first
        for s in sorted(self.substate):
            if self.sub(s).dirty():
                return True
        # check current working dir
        return ((merge and self.p2()) or
                (branch and self.branch() != self.p1().branch()) or
                self.modified() or self.added() or self.removed() or
                (missing and self.deleted()))

    def add(self, list, prefix=""):
        join = lambda f: os.path.join(prefix, f)
        wlock = self._repo.wlock()
        ui, ds = self._repo.ui, self._repo.dirstate
        try:
            rejected = []
            lstat = self._repo.wvfs.lstat
            for f in list:
                scmutil.checkportable(ui, join(f))
                try:
                    st = lstat(f)
                except OSError:
                    ui.warn(_("%s does not exist!\n") % join(f))
                    rejected.append(f)
                    continue
                if st.st_size > 10000000:
                    ui.warn(_("%s: up to %d MB of RAM may be required "
                              "to manage this file\n"
                              "(use 'hg revert %s' to cancel the "
                              "pending addition)\n")
                              % (f, 3 * st.st_size // 1000000, join(f)))
                if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
                    ui.warn(_("%s not added: only files and symlinks "
                              "supported currently\n") % join(f))
                    rejected.append(f)
                elif ds[f] in 'amn':
                    ui.warn(_("%s already tracked!\n") % join(f))
                elif ds[f] == 'r':
                    ds.normallookup(f)
                else:
                    ds.add(f)
            return rejected
        finally:
            wlock.release()

    def forget(self, files, prefix=""):
        join = lambda f: os.path.join(prefix, f)
        wlock = self._repo.wlock()
        try:
            rejected = []
            for f in files:
                if f not in self._repo.dirstate:
                    self._repo.ui.warn(_("%s not tracked!\n") % join(f))
                    rejected.append(f)
                elif self._repo.dirstate[f] != 'a':
                    self._repo.dirstate.remove(f)
                else:
                    self._repo.dirstate.drop(f)
            return rejected
        finally:
            wlock.release()

    def undelete(self, list):
        pctxs = self.parents()
        wlock = self._repo.wlock()
        try:
            for f in list:
                if self._repo.dirstate[f] != 'r':
                    self._repo.ui.warn(_("%s not removed!\n") % f)
                else:
                    fctx = f in pctxs[0] and pctxs[0][f] or pctxs[1][f]
                    t = fctx.data()
                    self._repo.wwrite(f, t, fctx.flags())
                    self._repo.dirstate.normal(f)
        finally:
            wlock.release()

    def copy(self, source, dest):
        try:
            st = self._repo.wvfs.lstat(dest)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            self._repo.ui.warn(_("%s does not exist!\n") % dest)
            return
        if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
            self._repo.ui.warn(_("copy failed: %s is not a file or a "
                                 "symbolic link\n") % dest)
        else:
            wlock = self._repo.wlock()
            try:
                if self._repo.dirstate[dest] in '?':
                    self._repo.dirstate.add(dest)
                elif self._repo.dirstate[dest] in 'r':
                    self._repo.dirstate.normallookup(dest)
                self._repo.dirstate.copy(source, dest)
            finally:
                wlock.release()

    def match(self, pats=[], include=None, exclude=None, default='glob',
              listsubrepos=False, badfn=None):
        r = self._repo

        # Only a case insensitive filesystem needs magic to translate user input
        # to actual case in the filesystem.
        if not util.checkcase(r.root):
            return matchmod.icasefsmatcher(r.root, r.getcwd(), pats, include,
                                           exclude, default, r.auditor, self,
                                           listsubrepos=listsubrepos,
                                           badfn=badfn)
        return matchmod.match(r.root, r.getcwd(), pats,
                              include, exclude, default,
                              auditor=r.auditor, ctx=self,
                              listsubrepos=listsubrepos, badfn=badfn)

    def _filtersuspectsymlink(self, files):
        if not files or self._repo.dirstate._checklink:
            return files

        # Symlink placeholders may get non-symlink-like contents
        # via user error or dereferencing by NFS or Samba servers,
        # so we filter out any placeholders that don't look like a
        # symlink
        sane = []
        for f in files:
            if self.flags(f) == 'l':
                d = self[f].data()
                if d == '' or len(d) >= 1024 or '\n' in d or util.binary(d):
                    self._repo.ui.debug('ignoring suspect symlink placeholder'
                                        ' "%s"\n' % f)
                    continue
            sane.append(f)
        return sane

    def _checklookup(self, files):
        # check for any possibly clean files
        if not files:
            return [], []

        modified = []
        fixup = []
        pctx = self._parents[0]
        # do a full compare of any files that might have changed
        for f in sorted(files):
            if (f not in pctx or self.flags(f) != pctx.flags(f)
                or pctx[f].cmp(self[f])):
                modified.append(f)
            else:
                fixup.append(f)

        # update dirstate for files that are actually clean
        if fixup:
            try:
                # updating the dirstate is optional
                # so we don't wait on the lock
                # wlock can invalidate the dirstate, so cache normal _after_
                # taking the lock
                wlock = self._repo.wlock(False)
                normal = self._repo.dirstate.normal
                try:
                    for f in fixup:
                        normal(f)
                    # write changes out explicitly, because nesting
                    # wlock at runtime may prevent 'wlock.release()'
                    # below from doing so for subsequent changing files
                    self._repo.dirstate.write()
                finally:
                    wlock.release()
            except error.LockError:
                pass
        return modified, fixup

    def _manifestmatches(self, match, s):
        """Slow path for workingctx

        The fast path is when we compare the working directory to its parent
        which means this function is comparing with a non-parent; therefore we
        need to build a manifest and return what matches.
        """
        mf = self._repo['.']._manifestmatches(match, s)
        for f in s.modified + s.added:
            mf[f] = _newnode
            mf.setflag(f, self.flags(f))
        for f in s.removed:
            if f in mf:
                del mf[f]
        return mf

    def _dirstatestatus(self, match=None, ignored=False, clean=False,
                        unknown=False):
        '''Gets the status from the dirstate -- internal use only.'''
        listignored, listclean, listunknown = ignored, clean, unknown
        match = match or matchmod.always(self._repo.root, self._repo.getcwd())
        subrepos = []
        if '.hgsub' in self:
            subrepos = sorted(self.substate)
        cmp, s = self._repo.dirstate.status(match, subrepos, listignored,
                                            listclean, listunknown)

        # check for any possibly clean files
        if cmp:
            modified2, fixup = self._checklookup(cmp)
            s.modified.extend(modified2)

            # update dirstate for files that are actually clean
            if fixup and listclean:
                s.clean.extend(fixup)

        if match.always():
            # cache for performance
            if s.unknown or s.ignored or s.clean:
                # "_status" is cached with list*=False in the normal route
                self._status = scmutil.status(s.modified, s.added, s.removed,
                                              s.deleted, [], [], [])
            else:
                self._status = s

        return s

    def _buildstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """build a status with respect to another context

        This includes logic for maintaining the fast path of status when
        comparing the working directory against its parent, which is to skip
        building a new manifest if self (working directory) is not comparing
        against its parent (repo['.']).
        """
        s = self._dirstatestatus(match, listignored, listclean, listunknown)
        # Filter out symlinks that, in the case of FAT32 and NTFS filesystems,
        # might have accidentally ended up with the entire contents of the file
        # they are supposed to be linking to.
        s.modified[:] = self._filtersuspectsymlink(s.modified)
        if other != self._repo['.']:
            s = super(workingctx, self)._buildstatus(other, s, match,
                                                     listignored, listclean,
                                                     listunknown)
        return s

    def _matchstatus(self, other, match):
        """override the match method with a filter for directory patterns

        We use inheritance to customize the match.bad method only in cases of
        workingctx since it belongs only to the working directory when
        comparing against the parent changeset.

        If we aren't comparing against the working directory's parent, then we
        just use the default match object sent to us.
        """
        superself = super(workingctx, self)
        match = superself._matchstatus(other, match)
        if other != self._repo['.']:
            def bad(f, msg):
                # 'f' may be a directory pattern from 'match.files()',
                # so 'f not in ctx1' is not enough
                if f not in other and not other.hasdir(f):
                    self._repo.ui.warn('%s: %s\n' %
                                       (self._repo.dirstate.pathto(f), msg))
            match.bad = bad
        return match

class committablefilectx(basefilectx):
    """A committablefilectx provides common functionality for a file context
    that wants the ability to commit, e.g. workingfilectx or memfilectx."""
    def __init__(self, repo, path, filelog=None, ctx=None):
        self._repo = repo
        self._path = path
        self._changeid = None
        self._filerev = self._filenode = None

        if filelog is not None:
            self._filelog = filelog
        if ctx:
            self._changectx = ctx

    def __nonzero__(self):
        return True

    def linkrev(self):
        # linked to self._changectx no matter if file is modified or not
        return self.rev()

    def parents(self):
        '''return parent filectxs, following copies if necessary'''
        def filenode(ctx, path):
            return ctx._manifest.get(path, nullid)

        path = self._path
        fl = self._filelog
        pcl = self._changectx._parents
        renamed = self.renamed()

        if renamed:
            pl = [renamed + (None,)]
        else:
            pl = [(path, filenode(pcl[0], path), fl)]

        for pc in pcl[1:]:
            pl.append((path, filenode(pc, path), fl))

        return [self._parentfilectx(p, fileid=n, filelog=l)
                for p, n, l in pl if n != nullid]

    def children(self):
        return []

class workingfilectx(committablefilectx):
    """A workingfilectx object makes access to data related to a particular
       file in the working directory convenient."""
    def __init__(self, repo, path, filelog=None, workingctx=None):
        super(workingfilectx, self).__init__(repo, path, filelog, workingctx)

    @propertycache
    def _changectx(self):
        return workingctx(self._repo)

    def data(self):
        return self._repo.wread(self._path)
    def renamed(self):
        rp = self._repo.dirstate.copied(self._path)
        if not rp:
            return None
        return rp, self._changectx._parents[0]._manifest.get(rp, nullid)

    def size(self):
        return self._repo.wvfs.lstat(self._path).st_size
    def date(self):
        t, tz = self._changectx.date()
        try:
            return (int(self._repo.wvfs.lstat(self._path).st_mtime), tz)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            return (t, tz)

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        # fctx should be a filectx (not a workingfilectx)
        # invert comparison to reuse the same code path
        return fctx.cmp(self)

    def remove(self, ignoremissing=False):
        """wraps unlink for a repo's working directory"""
        util.unlinkpath(self._repo.wjoin(self._path), ignoremissing)

    def write(self, data, flags):
        """wraps repo.wwrite"""
        self._repo.wwrite(self._path, data, flags)

class workingcommitctx(workingctx):
    """A workingcommitctx object makes access to data related to
    the revision being committed convenient.

    This hides changes in the working directory, if they aren't
    committed in this context.
    """
    def __init__(self, repo, changes,
                 text="", user=None, date=None, extra=None):
        super(workingctx, self).__init__(repo, text, user, date, extra,
                                         changes)

    def _dirstatestatus(self, match=None, ignored=False, clean=False,
                        unknown=False):
        """Return matched files only in ``self._status``

        Uncommitted files appear "clean" via this context, even if
        they aren't actually so in the working directory.
        """
        match = match or matchmod.always(self._repo.root, self._repo.getcwd())
        if clean:
            clean = [f for f in self._manifest if f not in self._changedset]
        else:
            clean = []
        return scmutil.status([f for f in self._status.modified if match(f)],
                              [f for f in self._status.added if match(f)],
                              [f for f in self._status.removed if match(f)],
                              [], [], [], clean)

    @propertycache
    def _changedset(self):
        """Return the set of files changed in this context
        """
        changed = set(self._status.modified)
        changed.update(self._status.added)
        changed.update(self._status.removed)
        return changed

class memctx(committablectx):
    """Use memctx to perform in-memory commits via localrepo.commitctx().

    Revision information is supplied at initialization time while
    related files data and is made available through a callback
    mechanism.  'repo' is the current localrepo, 'parents' is a
    sequence of two parent revisions identifiers (pass None for every
    missing parent), 'text' is the commit message and 'files' lists
    names of files touched by the revision (normalized and relative to
    repository root).

    filectxfn(repo, memctx, path) is a callable receiving the
    repository, the current memctx object and the normalized path of
    requested file, relative to repository root. It is fired by the
    commit function for every file in 'files', but calls order is
    undefined. If the file is available in the revision being
    committed (updated or added), filectxfn returns a memfilectx
    object. If the file was removed, filectxfn raises an
    IOError. Moved files are represented by marking the source file
    removed and the new file added with copy information (see
    memfilectx).

    user receives the committer name and defaults to current
    repository username, date is the commit date in any format
    supported by util.parsedate() and defaults to current date, extra
    is a dictionary of metadata or is left empty.
    """

    # Mercurial <= 3.1 expects the filectxfn to raise IOError for missing files.
    # Extensions that need to retain compatibility across Mercurial 3.1 can use
    # this field to determine what to do in filectxfn.
    _returnnoneformissingfiles = True

    def __init__(self, repo, parents, text, files, filectxfn, user=None,
                 date=None, extra=None, editor=False):
        super(memctx, self).__init__(repo, text, user, date, extra)
        self._rev = None
        self._node = None
        parents = [(p or nullid) for p in parents]
        p1, p2 = parents
        self._parents = [changectx(self._repo, p) for p in (p1, p2)]
        files = sorted(set(files))
        self._files = files
        self.substate = {}

        # if store is not callable, wrap it in a function
        if not callable(filectxfn):
            def getfilectx(repo, memctx, path):
                fctx = filectxfn[path]
                # this is weird but apparently we only keep track of one parent
                # (why not only store that instead of a tuple?)
                copied = fctx.renamed()
                if copied:
                    copied = copied[0]
                return memfilectx(repo, path, fctx.data(),
                                  islink=fctx.islink(), isexec=fctx.isexec(),
                                  copied=copied, memctx=memctx)
            self._filectxfn = getfilectx
        else:
            # "util.cachefunc" reduces invocation of possibly expensive
            # "filectxfn" for performance (e.g. converting from another VCS)
            self._filectxfn = util.cachefunc(filectxfn)

        if extra:
            self._extra = extra.copy()
        else:
            self._extra = {}

        if self._extra.get('branch', '') == '':
            self._extra['branch'] = 'default'

        if editor:
            self._text = editor(self._repo, self, [])
            self._repo.savecommitmessage(self._text)

    def filectx(self, path, filelog=None):
        """get a file context from the working directory

        Returns None if file doesn't exist and should be removed."""
        return self._filectxfn(self._repo, self, path)

    def commit(self):
        """commit context to the repo"""
        return self._repo.commitctx(self)

    @propertycache
    def _manifest(self):
        """generate a manifest based on the return values of filectxfn"""

        # keep this simple for now; just worry about p1
        pctx = self._parents[0]
        man = pctx.manifest().copy()

        for f in self._status.modified:
            p1node = nullid
            p2node = nullid
            p = pctx[f].parents() # if file isn't in pctx, check p2?
            if len(p) > 0:
                p1node = p[0].node()
                if len(p) > 1:
                    p2node = p[1].node()
            man[f] = revlog.hash(self[f].data(), p1node, p2node)

        for f in self._status.added:
            man[f] = revlog.hash(self[f].data(), nullid, nullid)

        for f in self._status.removed:
            if f in man:
                del man[f]

        return man

    @propertycache
    def _status(self):
        """Calculate exact status from ``files`` specified at construction
        """
        man1 = self.p1().manifest()
        p2 = self._parents[1]
        # "1 < len(self._parents)" can't be used for checking
        # existence of the 2nd parent, because "memctx._parents" is
        # explicitly initialized by the list, of which length is 2.
        if p2.node() != nullid:
            man2 = p2.manifest()
            managing = lambda f: f in man1 or f in man2
        else:
            managing = lambda f: f in man1

        modified, added, removed = [], [], []
        for f in self._files:
            if not managing(f):
                added.append(f)
            elif self[f]:
                modified.append(f)
            else:
                removed.append(f)

        return scmutil.status(modified, added, removed, [], [], [], [])

class memfilectx(committablefilectx):
    """memfilectx represents an in-memory file to commit.

    See memctx and committablefilectx for more details.
    """
    def __init__(self, repo, path, data, islink=False,
                 isexec=False, copied=None, memctx=None):
        """
        path is the normalized file path relative to repository root.
        data is the file content as a string.
        islink is True if the file is a symbolic link.
        isexec is True if the file is executable.
        copied is the source file path if current file was copied in the
        revision being committed, or None."""
        super(memfilectx, self).__init__(repo, path, None, memctx)
        self._data = data
        self._flags = (islink and 'l' or '') + (isexec and 'x' or '')
        self._copied = None
        if copied:
            self._copied = (copied, nullid)

    def data(self):
        return self._data
    def size(self):
        return len(self.data())
    def flags(self):
        return self._flags
    def renamed(self):
        return self._copied

    def remove(self, ignoremissing=False):
        """wraps unlink for a repo's working directory"""
        # need to figure out what to do here
        del self._changectx[self._path]

    def write(self, data, flags):
        """wraps repo.wwrite"""
        self._data = data
