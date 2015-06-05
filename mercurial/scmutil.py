# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from mercurial.node import nullrev
import util, error, osutil, revset, similar, encoding, phases
import pathutil
import match as matchmod
import os, errno, re, glob, tempfile, shutil, stat, inspect

if os.name == 'nt':
    import scmwindows as scmplatform
else:
    import scmposix as scmplatform

systemrcpath = scmplatform.systemrcpath
userrcpath = scmplatform.userrcpath

class status(tuple):
    '''Named tuple with a list of files per status. The 'deleted', 'unknown'
       and 'ignored' properties are only relevant to the working copy.
    '''

    __slots__ = ()

    def __new__(cls, modified, added, removed, deleted, unknown, ignored,
                clean):
        return tuple.__new__(cls, (modified, added, removed, deleted, unknown,
                                   ignored, clean))

    @property
    def modified(self):
        '''files that have been modified'''
        return self[0]

    @property
    def added(self):
        '''files that have been added'''
        return self[1]

    @property
    def removed(self):
        '''files that have been removed'''
        return self[2]

    @property
    def deleted(self):
        '''files that are in the dirstate, but have been deleted from the
           working copy (aka "missing")
        '''
        return self[3]

    @property
    def unknown(self):
        '''files not in the dirstate that are not ignored'''
        return self[4]

    @property
    def ignored(self):
        '''files not in the dirstate that are ignored (by _dirignore())'''
        return self[5]

    @property
    def clean(self):
        '''files that have not been modified'''
        return self[6]

    def __repr__(self, *args, **kwargs):
        return (('<status modified=%r, added=%r, removed=%r, deleted=%r, '
                 'unknown=%r, ignored=%r, clean=%r>') % self)

def itersubrepos(ctx1, ctx2):
    """find subrepos in ctx1 or ctx2"""
    # Create a (subpath, ctx) mapping where we prefer subpaths from
    # ctx1. The subpaths from ctx2 are important when the .hgsub file
    # has been modified (in ctx2) but not yet committed (in ctx1).
    subpaths = dict.fromkeys(ctx2.substate, ctx2)
    subpaths.update(dict.fromkeys(ctx1.substate, ctx1))

    missing = set()

    for subpath in ctx2.substate:
        if subpath not in ctx1.substate:
            del subpaths[subpath]
            missing.add(subpath)

    for subpath, ctx in sorted(subpaths.iteritems()):
        yield subpath, ctx.sub(subpath)

    # Yield an empty subrepo based on ctx1 for anything only in ctx2.  That way,
    # status and diff will have an accurate result when it does
    # 'sub.{status|diff}(rev2)'.  Otherwise, the ctx2 subrepo is compared
    # against itself.
    for subpath in missing:
        yield subpath, ctx2.nullsub(subpath, ctx1)

def nochangesfound(ui, repo, excluded=None):
    '''Report no changes for push/pull, excluded is None or a list of
    nodes excluded from the push/pull.
    '''
    secretlist = []
    if excluded:
        for n in excluded:
            if n not in repo:
                # discovery should not have included the filtered revision,
                # we have to explicitly exclude it until discovery is cleanup.
                continue
            ctx = repo[n]
            if ctx.phase() >= phases.secret and not ctx.extinct():
                secretlist.append(n)

    if secretlist:
        ui.status(_("no changes found (ignored %d secret changesets)\n")
                  % len(secretlist))
    else:
        ui.status(_("no changes found\n"))

def checknewlabel(repo, lbl, kind):
    # Do not use the "kind" parameter in ui output.
    # It makes strings difficult to translate.
    if lbl in ['tip', '.', 'null']:
        raise util.Abort(_("the name '%s' is reserved") % lbl)
    for c in (':', '\0', '\n', '\r'):
        if c in lbl:
            raise util.Abort(_("%r cannot be used in a name") % c)
    try:
        int(lbl)
        raise util.Abort(_("cannot use an integer as a name"))
    except ValueError:
        pass

def checkfilename(f):
    '''Check that the filename f is an acceptable filename for a tracked file'''
    if '\r' in f or '\n' in f:
        raise util.Abort(_("'\\n' and '\\r' disallowed in filenames: %r") % f)

def checkportable(ui, f):
    '''Check if filename f is portable and warn or abort depending on config'''
    checkfilename(f)
    abort, warn = checkportabilityalert(ui)
    if abort or warn:
        msg = util.checkwinfilename(f)
        if msg:
            msg = "%s: %r" % (msg, f)
            if abort:
                raise util.Abort(msg)
            ui.warn(_("warning: %s\n") % msg)

def checkportabilityalert(ui):
    '''check if the user's config requests nothing, a warning, or abort for
    non-portable filenames'''
    val = ui.config('ui', 'portablefilenames', 'warn')
    lval = val.lower()
    bval = util.parsebool(val)
    abort = os.name == 'nt' or lval == 'abort'
    warn = bval or lval == 'warn'
    if bval is None and not (warn or abort or lval == 'ignore'):
        raise error.ConfigError(
            _("ui.portablefilenames value is invalid ('%s')") % val)
    return abort, warn

class casecollisionauditor(object):
    def __init__(self, ui, abort, dirstate):
        self._ui = ui
        self._abort = abort
        allfiles = '\0'.join(dirstate._map)
        self._loweredfiles = set(encoding.lower(allfiles).split('\0'))
        self._dirstate = dirstate
        # The purpose of _newfiles is so that we don't complain about
        # case collisions if someone were to call this object with the
        # same filename twice.
        self._newfiles = set()

    def __call__(self, f):
        if f in self._newfiles:
            return
        fl = encoding.lower(f)
        if fl in self._loweredfiles and f not in self._dirstate:
            msg = _('possible case-folding collision for %s') % f
            if self._abort:
                raise util.Abort(msg)
            self._ui.warn(_("warning: %s\n") % msg)
        self._loweredfiles.add(fl)
        self._newfiles.add(f)

def develwarn(tui, msg):
    """issue a developer warning message"""
    msg = 'devel-warn: ' + msg
    if tui.tracebackflag:
        util.debugstacktrace(msg, 2)
    else:
        curframe = inspect.currentframe()
        calframe = inspect.getouterframes(curframe, 2)
        tui.write_err('%s at: %s:%s (%s)\n' % ((msg,) + calframe[2][1:4]))

def filteredhash(repo, maxrev):
    """build hash of filtered revisions in the current repoview.

    Multiple caches perform up-to-date validation by checking that the
    tiprev and tipnode stored in the cache file match the current repository.
    However, this is not sufficient for validating repoviews because the set
    of revisions in the view may change without the repository tiprev and
    tipnode changing.

    This function hashes all the revs filtered from the view and returns
    that SHA-1 digest.
    """
    cl = repo.changelog
    if not cl.filteredrevs:
        return None
    key = None
    revs = sorted(r for r in cl.filteredrevs if r <= maxrev)
    if revs:
        s = util.sha1()
        for rev in revs:
            s.update('%s;' % rev)
        key = s.digest()
    return key

class abstractvfs(object):
    """Abstract base class; cannot be instantiated"""

    def __init__(self, *args, **kwargs):
        '''Prevent instantiation; don't call this from subclasses.'''
        raise NotImplementedError('attempted instantiating ' + str(type(self)))

    def tryread(self, path):
        '''gracefully return an empty string for missing files'''
        try:
            return self.read(path)
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
        return ""

    def tryreadlines(self, path, mode='rb'):
        '''gracefully return an empty array for missing files'''
        try:
            return self.readlines(path, mode=mode)
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
        return []

    def open(self, path, mode="r", text=False, atomictemp=False,
             notindexed=False):
        '''Open ``path`` file, which is relative to vfs root.

        Newly created directories are marked as "not to be indexed by
        the content indexing service", if ``notindexed`` is specified
        for "write" mode access.
        '''
        self.open = self.__call__
        return self.__call__(path, mode, text, atomictemp, notindexed)

    def read(self, path):
        fp = self(path, 'rb')
        try:
            return fp.read()
        finally:
            fp.close()

    def readlines(self, path, mode='rb'):
        fp = self(path, mode=mode)
        try:
            return fp.readlines()
        finally:
            fp.close()

    def write(self, path, data):
        fp = self(path, 'wb')
        try:
            return fp.write(data)
        finally:
            fp.close()

    def writelines(self, path, data, mode='wb', notindexed=False):
        fp = self(path, mode=mode, notindexed=notindexed)
        try:
            return fp.writelines(data)
        finally:
            fp.close()

    def append(self, path, data):
        fp = self(path, 'ab')
        try:
            return fp.write(data)
        finally:
            fp.close()

    def chmod(self, path, mode):
        return os.chmod(self.join(path), mode)

    def exists(self, path=None):
        return os.path.exists(self.join(path))

    def fstat(self, fp):
        return util.fstat(fp)

    def isdir(self, path=None):
        return os.path.isdir(self.join(path))

    def isfile(self, path=None):
        return os.path.isfile(self.join(path))

    def islink(self, path=None):
        return os.path.islink(self.join(path))

    def reljoin(self, *paths):
        """join various elements of a path together (as os.path.join would do)

        The vfs base is not injected so that path stay relative. This exists
        to allow handling of strange encoding if needed."""
        return os.path.join(*paths)

    def split(self, path):
        """split top-most element of a path (as os.path.split would do)

        This exists to allow handling of strange encoding if needed."""
        return os.path.split(path)

    def lexists(self, path=None):
        return os.path.lexists(self.join(path))

    def lstat(self, path=None):
        return os.lstat(self.join(path))

    def listdir(self, path=None):
        return os.listdir(self.join(path))

    def makedir(self, path=None, notindexed=True):
        return util.makedir(self.join(path), notindexed)

    def makedirs(self, path=None, mode=None):
        return util.makedirs(self.join(path), mode)

    def makelock(self, info, path):
        return util.makelock(info, self.join(path))

    def mkdir(self, path=None):
        return os.mkdir(self.join(path))

    def mkstemp(self, suffix='', prefix='tmp', dir=None, text=False):
        fd, name = tempfile.mkstemp(suffix=suffix, prefix=prefix,
                                    dir=self.join(dir), text=text)
        dname, fname = util.split(name)
        if dir:
            return fd, os.path.join(dir, fname)
        else:
            return fd, fname

    def readdir(self, path=None, stat=None, skip=None):
        return osutil.listdir(self.join(path), stat, skip)

    def readlock(self, path):
        return util.readlock(self.join(path))

    def rename(self, src, dst):
        return util.rename(self.join(src), self.join(dst))

    def readlink(self, path):
        return os.readlink(self.join(path))

    def removedirs(self, path=None):
        """Remove a leaf directory and all empty intermediate ones
        """
        return util.removedirs(self.join(path))

    def rmtree(self, path=None, ignore_errors=False, forcibly=False):
        """Remove a directory tree recursively

        If ``forcibly``, this tries to remove READ-ONLY files, too.
        """
        if forcibly:
            def onerror(function, path, excinfo):
                if function is not os.remove:
                    raise
                # read-only files cannot be unlinked under Windows
                s = os.stat(path)
                if (s.st_mode & stat.S_IWRITE) != 0:
                    raise
                os.chmod(path, stat.S_IMODE(s.st_mode) | stat.S_IWRITE)
                os.remove(path)
        else:
            onerror = None
        return shutil.rmtree(self.join(path),
                             ignore_errors=ignore_errors, onerror=onerror)

    def setflags(self, path, l, x):
        return util.setflags(self.join(path), l, x)

    def stat(self, path=None):
        return os.stat(self.join(path))

    def unlink(self, path=None):
        return util.unlink(self.join(path))

    def unlinkpath(self, path=None, ignoremissing=False):
        return util.unlinkpath(self.join(path), ignoremissing)

    def utime(self, path=None, t=None):
        return os.utime(self.join(path), t)

    def walk(self, path=None, onerror=None):
        """Yield (dirpath, dirs, files) tuple for each directories under path

        ``dirpath`` is relative one from the root of this vfs. This
        uses ``os.sep`` as path separator, even you specify POSIX
        style ``path``.

        "The root of this vfs" is represented as empty ``dirpath``.
        """
        root = os.path.normpath(self.join(None))
        # when dirpath == root, dirpath[prefixlen:] becomes empty
        # because len(dirpath) < prefixlen.
        prefixlen = len(pathutil.normasprefix(root))
        for dirpath, dirs, files in os.walk(self.join(path), onerror=onerror):
            yield (dirpath[prefixlen:], dirs, files)

class vfs(abstractvfs):
    '''Operate files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.
    '''
    def __init__(self, base, audit=True, expandpath=False, realpath=False):
        if expandpath:
            base = util.expandpath(base)
        if realpath:
            base = os.path.realpath(base)
        self.base = base
        self._setmustaudit(audit)
        self.createmode = None
        self._trustnlink = None

    def _getmustaudit(self):
        return self._audit

    def _setmustaudit(self, onoff):
        self._audit = onoff
        if onoff:
            self.audit = pathutil.pathauditor(self.base)
        else:
            self.audit = util.always

    mustaudit = property(_getmustaudit, _setmustaudit)

    @util.propertycache
    def _cansymlink(self):
        return util.checklink(self.base)

    @util.propertycache
    def _chmod(self):
        return util.checkexec(self.base)

    def _fixfilemode(self, name):
        if self.createmode is None or not self._chmod:
            return
        os.chmod(name, self.createmode & 0666)

    def __call__(self, path, mode="r", text=False, atomictemp=False,
                 notindexed=False):
        '''Open ``path`` file, which is relative to vfs root.

        Newly created directories are marked as "not to be indexed by
        the content indexing service", if ``notindexed`` is specified
        for "write" mode access.
        '''
        if self._audit:
            r = util.checkosfilename(path)
            if r:
                raise util.Abort("%s: %r" % (r, path))
        self.audit(path)
        f = self.join(path)

        if not text and "b" not in mode:
            mode += "b" # for that other OS

        nlink = -1
        if mode not in ('r', 'rb'):
            dirname, basename = util.split(f)
            # If basename is empty, then the path is malformed because it points
            # to a directory. Let the posixfile() call below raise IOError.
            if basename:
                if atomictemp:
                    util.ensuredirs(dirname, self.createmode, notindexed)
                    return util.atomictempfile(f, mode, self.createmode)
                try:
                    if 'w' in mode:
                        util.unlink(f)
                        nlink = 0
                    else:
                        # nlinks() may behave differently for files on Windows
                        # shares if the file is open.
                        fd = util.posixfile(f)
                        nlink = util.nlinks(f)
                        if nlink < 1:
                            nlink = 2 # force mktempcopy (issue1922)
                        fd.close()
                except (OSError, IOError), e:
                    if e.errno != errno.ENOENT:
                        raise
                    nlink = 0
                    util.ensuredirs(dirname, self.createmode, notindexed)
                if nlink > 0:
                    if self._trustnlink is None:
                        self._trustnlink = nlink > 1 or util.checknlink(f)
                    if nlink > 1 or not self._trustnlink:
                        util.rename(util.mktempcopy(f), f)
        fp = util.posixfile(f, mode)
        if nlink == 0:
            self._fixfilemode(f)
        return fp

    def symlink(self, src, dst):
        self.audit(dst)
        linkname = self.join(dst)
        try:
            os.unlink(linkname)
        except OSError:
            pass

        util.ensuredirs(os.path.dirname(linkname), self.createmode)

        if self._cansymlink:
            try:
                os.symlink(src, linkname)
            except OSError, err:
                raise OSError(err.errno, _('could not symlink to %r: %s') %
                              (src, err.strerror), linkname)
        else:
            self.write(dst, src)

    def join(self, path, *insidef):
        if path:
            return os.path.join(self.base, path, *insidef)
        else:
            return self.base

opener = vfs

class auditvfs(object):
    def __init__(self, vfs):
        self.vfs = vfs

    def _getmustaudit(self):
        return self.vfs.mustaudit

    def _setmustaudit(self, onoff):
        self.vfs.mustaudit = onoff

    mustaudit = property(_getmustaudit, _setmustaudit)

class filtervfs(abstractvfs, auditvfs):
    '''Wrapper vfs for filtering filenames with a function.'''

    def __init__(self, vfs, filter):
        auditvfs.__init__(self, vfs)
        self._filter = filter

    def __call__(self, path, *args, **kwargs):
        return self.vfs(self._filter(path), *args, **kwargs)

    def join(self, path, *insidef):
        if path:
            return self.vfs.join(self._filter(self.vfs.reljoin(path, *insidef)))
        else:
            return self.vfs.join(path)

filteropener = filtervfs

class readonlyvfs(abstractvfs, auditvfs):
    '''Wrapper vfs preventing any writing.'''

    def __init__(self, vfs):
        auditvfs.__init__(self, vfs)

    def __call__(self, path, mode='r', *args, **kw):
        if mode not in ('r', 'rb'):
            raise util.Abort('this vfs is read only')
        return self.vfs(path, mode, *args, **kw)


def walkrepos(path, followsym=False, seen_dirs=None, recurse=False):
    '''yield every hg repository under path, always recursively.
    The recurse flag will only control recursion into repo working dirs'''
    def errhandler(err):
        if err.filename == path:
            raise err
    samestat = getattr(os.path, 'samestat', None)
    if followsym and samestat is not None:
        def adddir(dirlst, dirname):
            match = False
            dirstat = os.stat(dirname)
            for lstdirstat in dirlst:
                if samestat(dirstat, lstdirstat):
                    match = True
                    break
            if not match:
                dirlst.append(dirstat)
            return not match
    else:
        followsym = False

    if (seen_dirs is None) and followsym:
        seen_dirs = []
        adddir(seen_dirs, path)
    for root, dirs, files in os.walk(path, topdown=True, onerror=errhandler):
        dirs.sort()
        if '.hg' in dirs:
            yield root # found a repository
            qroot = os.path.join(root, '.hg', 'patches')
            if os.path.isdir(os.path.join(qroot, '.hg')):
                yield qroot # we have a patch queue repo here
            if recurse:
                # avoid recursing inside the .hg directory
                dirs.remove('.hg')
            else:
                dirs[:] = [] # don't descend further
        elif followsym:
            newdirs = []
            for d in dirs:
                fname = os.path.join(root, d)
                if adddir(seen_dirs, fname):
                    if os.path.islink(fname):
                        for hgname in walkrepos(fname, True, seen_dirs):
                            yield hgname
                    else:
                        newdirs.append(d)
            dirs[:] = newdirs

def osrcpath():
    '''return default os-specific hgrc search path'''
    path = []
    defaultpath = os.path.join(util.datapath, 'default.d')
    if os.path.isdir(defaultpath):
        for f, kind in osutil.listdir(defaultpath):
            if f.endswith('.rc'):
                path.append(os.path.join(defaultpath, f))
    path.extend(systemrcpath())
    path.extend(userrcpath())
    path = [os.path.normpath(f) for f in path]
    return path

_rcpath = None

def rcpath():
    '''return hgrc search path. if env var HGRCPATH is set, use it.
    for each item in path, if directory, use files ending in .rc,
    else use item.
    make HGRCPATH empty to only look in .hg/hgrc of current repo.
    if no HGRCPATH, use default os-specific path.'''
    global _rcpath
    if _rcpath is None:
        if 'HGRCPATH' in os.environ:
            _rcpath = []
            for p in os.environ['HGRCPATH'].split(os.pathsep):
                if not p:
                    continue
                p = util.expandpath(p)
                if os.path.isdir(p):
                    for f, kind in osutil.listdir(p):
                        if f.endswith('.rc'):
                            _rcpath.append(os.path.join(p, f))
                else:
                    _rcpath.append(p)
        else:
            _rcpath = osrcpath()
    return _rcpath

def intrev(repo, rev):
    """Return integer for a given revision that can be used in comparison or
    arithmetic operation"""
    if rev is None:
        return len(repo)
    return rev

def revsingle(repo, revspec, default='.'):
    if not revspec and revspec != 0:
        return repo[default]

    l = revrange(repo, [revspec])
    if not l:
        raise util.Abort(_('empty revision set'))
    return repo[l.last()]

def revpair(repo, revs):
    if not revs:
        return repo.dirstate.p1(), None

    l = revrange(repo, revs)

    if not l:
        first = second = None
    elif l.isascending():
        first = l.min()
        second = l.max()
    elif l.isdescending():
        first = l.max()
        second = l.min()
    else:
        first = l.first()
        second = l.last()

    if first is None:
        raise util.Abort(_('empty revision range'))

    if first == second and len(revs) == 1 and _revrangesep not in revs[0]:
        return repo.lookup(first), None

    return repo.lookup(first), repo.lookup(second)

_revrangesep = ':'

def revrange(repo, revs):
    """Yield revision as strings from a list of revision specifications."""

    def revfix(repo, val, defval):
        if not val and val != 0 and defval is not None:
            return defval
        return repo[val].rev()

    subsets = []

    revsetaliases = [alias for (alias, _) in
                     repo.ui.configitems("revsetalias")]

    for spec in revs:
        # attempt to parse old-style ranges first to deal with
        # things like old-tag which contain query metacharacters
        try:
            # ... except for revset aliases without arguments. These
            # should be parsed as soon as possible, because they might
            # clash with a hash prefix.
            if spec in revsetaliases:
                raise error.RepoLookupError

            if isinstance(spec, int):
                subsets.append(revset.baseset([spec]))
                continue

            if _revrangesep in spec:
                start, end = spec.split(_revrangesep, 1)
                if start in revsetaliases or end in revsetaliases:
                    raise error.RepoLookupError

                start = revfix(repo, start, 0)
                end = revfix(repo, end, len(repo) - 1)
                if end == nullrev and start < 0:
                    start = nullrev
                if start < end:
                    l = revset.spanset(repo, start, end + 1)
                else:
                    l = revset.spanset(repo, start, end - 1)
                subsets.append(l)
                continue
            elif spec and spec in repo: # single unquoted rev
                rev = revfix(repo, spec, None)
                subsets.append(revset.baseset([rev]))
                continue
        except error.RepoLookupError:
            pass

        # fall through to new-style queries if old-style fails
        m = revset.match(repo.ui, spec, repo)
        subsets.append(m(repo))

    return revset._combinesets(subsets)

def expandpats(pats):
    '''Expand bare globs when running on windows.
    On posix we assume it already has already been done by sh.'''
    if not util.expandglobs:
        return list(pats)
    ret = []
    for kindpat in pats:
        kind, pat = matchmod._patsplit(kindpat, None)
        if kind is None:
            try:
                globbed = glob.glob(pat)
            except re.error:
                globbed = [pat]
            if globbed:
                ret.extend(globbed)
                continue
        ret.append(kindpat)
    return ret

def matchandpats(ctx, pats=[], opts={}, globbed=False, default='relpath',
                 badfn=None):
    '''Return a matcher and the patterns that were used.
    The matcher will warn about bad matches, unless an alternate badfn callback
    is provided.'''
    if pats == ("",):
        pats = []
    if not globbed and default == 'relpath':
        pats = expandpats(pats or [])

    def bad(f, msg):
        ctx.repo().ui.warn("%s: %s\n" % (m.rel(f), msg))

    if badfn is None:
        badfn = bad

    m = ctx.match(pats, opts.get('include'), opts.get('exclude'),
                  default, listsubrepos=opts.get('subrepos'), badfn=badfn)

    if m.always():
        pats = []
    return m, pats

def match(ctx, pats=[], opts={}, globbed=False, default='relpath', badfn=None):
    '''Return a matcher that will warn about bad matches.'''
    return matchandpats(ctx, pats, opts, globbed, default, badfn=badfn)[0]

def matchall(repo):
    '''Return a matcher that will efficiently match everything.'''
    return matchmod.always(repo.root, repo.getcwd())

def matchfiles(repo, files, badfn=None):
    '''Return a matcher that will efficiently match exactly these files.'''
    return matchmod.exact(repo.root, repo.getcwd(), files, badfn=badfn)

def addremove(repo, matcher, prefix, opts={}, dry_run=None, similarity=None):
    m = matcher
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)

    ret = 0
    join = lambda f: os.path.join(prefix, f)

    def matchessubrepo(matcher, subpath):
        if matcher.exact(subpath):
            return True
        for f in matcher.files():
            if f.startswith(subpath):
                return True
        return False

    wctx = repo[None]
    for subpath in sorted(wctx.substate):
        if opts.get('subrepos') or matchessubrepo(m, subpath):
            sub = wctx.sub(subpath)
            try:
                submatch = matchmod.narrowmatcher(subpath, m)
                if sub.addremove(submatch, prefix, opts, dry_run, similarity):
                    ret = 1
            except error.LookupError:
                repo.ui.status(_("skipping missing subrepository: %s\n")
                                 % join(subpath))

    rejected = []
    def badfn(f, msg):
        if f in m.files():
            m.bad(f, msg)
        rejected.append(f)

    badmatch = matchmod.badmatch(m, badfn)
    added, unknown, deleted, removed, forgotten = _interestingfiles(repo,
                                                                    badmatch)

    unknownset = set(unknown + forgotten)
    toprint = unknownset.copy()
    toprint.update(deleted)
    for abs in sorted(toprint):
        if repo.ui.verbose or not m.exact(abs):
            if abs in unknownset:
                status = _('adding %s\n') % m.uipath(abs)
            else:
                status = _('removing %s\n') % m.uipath(abs)
            repo.ui.status(status)

    renames = _findrenames(repo, m, added + unknown, removed + deleted,
                           similarity)

    if not dry_run:
        _markchanges(repo, unknown + forgotten, deleted, renames)

    for f in rejected:
        if f in m.files():
            return 1
    return ret

def marktouched(repo, files, similarity=0.0):
    '''Assert that files have somehow been operated upon. files are relative to
    the repo root.'''
    m = matchfiles(repo, files, badfn=lambda x, y: rejected.append(x))
    rejected = []

    added, unknown, deleted, removed, forgotten = _interestingfiles(repo, m)

    if repo.ui.verbose:
        unknownset = set(unknown + forgotten)
        toprint = unknownset.copy()
        toprint.update(deleted)
        for abs in sorted(toprint):
            if abs in unknownset:
                status = _('adding %s\n') % abs
            else:
                status = _('removing %s\n') % abs
            repo.ui.status(status)

    renames = _findrenames(repo, m, added + unknown, removed + deleted,
                           similarity)

    _markchanges(repo, unknown + forgotten, deleted, renames)

    for f in rejected:
        if f in m.files():
            return 1
    return 0

def _interestingfiles(repo, matcher):
    '''Walk dirstate with matcher, looking for files that addremove would care
    about.

    This is different from dirstate.status because it doesn't care about
    whether files are modified or clean.'''
    added, unknown, deleted, removed, forgotten = [], [], [], [], []
    audit_path = pathutil.pathauditor(repo.root)

    ctx = repo[None]
    dirstate = repo.dirstate
    walkresults = dirstate.walk(matcher, sorted(ctx.substate), True, False,
                                full=False)
    for abs, st in walkresults.iteritems():
        dstate = dirstate[abs]
        if dstate == '?' and audit_path.check(abs):
            unknown.append(abs)
        elif dstate != 'r' and not st:
            deleted.append(abs)
        elif dstate == 'r' and st:
            forgotten.append(abs)
        # for finding renames
        elif dstate == 'r' and not st:
            removed.append(abs)
        elif dstate == 'a':
            added.append(abs)

    return added, unknown, deleted, removed, forgotten

def _findrenames(repo, matcher, added, removed, similarity):
    '''Find renames from removed files to added ones.'''
    renames = {}
    if similarity > 0:
        for old, new, score in similar.findrenames(repo, added, removed,
                                                   similarity):
            if (repo.ui.verbose or not matcher.exact(old)
                or not matcher.exact(new)):
                repo.ui.status(_('recording removal of %s as rename to %s '
                                 '(%d%% similar)\n') %
                               (matcher.rel(old), matcher.rel(new),
                                score * 100))
            renames[new] = old
    return renames

def _markchanges(repo, unknown, deleted, renames):
    '''Marks the files in unknown as added, the files in deleted as removed,
    and the files in renames as copied.'''
    wctx = repo[None]
    wlock = repo.wlock()
    try:
        wctx.forget(deleted)
        wctx.add(unknown)
        for new, old in renames.iteritems():
            wctx.copy(old, new)
    finally:
        wlock.release()

def dirstatecopy(ui, repo, wctx, src, dst, dryrun=False, cwd=None):
    """Update the dirstate to reflect the intent of copying src to dst. For
    different reasons it might not end with dst being marked as copied from src.
    """
    origsrc = repo.dirstate.copied(src) or src
    if dst == origsrc: # copying back a copy?
        if repo.dirstate[dst] not in 'mn' and not dryrun:
            repo.dirstate.normallookup(dst)
    else:
        if repo.dirstate[origsrc] == 'a' and origsrc == src:
            if not ui.quiet:
                ui.warn(_("%s has not been committed yet, so no copy "
                          "data will be stored for %s.\n")
                        % (repo.pathto(origsrc, cwd), repo.pathto(dst, cwd)))
            if repo.dirstate[dst] in '?r' and not dryrun:
                wctx.add([dst])
        elif not dryrun:
            wctx.copy(origsrc, dst)

def readrequires(opener, supported):
    '''Reads and parses .hg/requires and checks if all entries found
    are in the list of supported features.'''
    requirements = set(opener.read("requires").splitlines())
    missings = []
    for r in requirements:
        if r not in supported:
            if not r or not r[0].isalnum():
                raise error.RequirementError(_(".hg/requires file is corrupt"))
            missings.append(r)
    missings.sort()
    if missings:
        raise error.RequirementError(
            _("repository requires features unknown to this Mercurial: %s")
            % " ".join(missings),
            hint=_("see http://mercurial.selenic.com/wiki/MissingRequirement"
                   " for more information"))
    return requirements

def writerequires(opener, requirements):
    reqfile = opener("requires", "w")
    for r in sorted(requirements):
        reqfile.write("%s\n" % r)
    reqfile.close()

class filecachesubentry(object):
    def __init__(self, path, stat):
        self.path = path
        self.cachestat = None
        self._cacheable = None

        if stat:
            self.cachestat = filecachesubentry.stat(self.path)

            if self.cachestat:
                self._cacheable = self.cachestat.cacheable()
            else:
                # None means we don't know yet
                self._cacheable = None

    def refresh(self):
        if self.cacheable():
            self.cachestat = filecachesubentry.stat(self.path)

    def cacheable(self):
        if self._cacheable is not None:
            return self._cacheable

        # we don't know yet, assume it is for now
        return True

    def changed(self):
        # no point in going further if we can't cache it
        if not self.cacheable():
            return True

        newstat = filecachesubentry.stat(self.path)

        # we may not know if it's cacheable yet, check again now
        if newstat and self._cacheable is None:
            self._cacheable = newstat.cacheable()

            # check again
            if not self._cacheable:
                return True

        if self.cachestat != newstat:
            self.cachestat = newstat
            return True
        else:
            return False

    @staticmethod
    def stat(path):
        try:
            return util.cachestat(path)
        except OSError, e:
            if e.errno != errno.ENOENT:
                raise

class filecacheentry(object):
    def __init__(self, paths, stat=True):
        self._entries = []
        for path in paths:
            self._entries.append(filecachesubentry(path, stat))

    def changed(self):
        '''true if any entry has changed'''
        for entry in self._entries:
            if entry.changed():
                return True
        return False

    def refresh(self):
        for entry in self._entries:
            entry.refresh()

class filecache(object):
    '''A property like decorator that tracks files under .hg/ for updates.

    Records stat info when called in _filecache.

    On subsequent calls, compares old stat info with new info, and recreates the
    object when any of the files changes, updating the new stat info in
    _filecache.

    Mercurial either atomic renames or appends for files under .hg,
    so to ensure the cache is reliable we need the filesystem to be able
    to tell us if a file has been replaced. If it can't, we fallback to
    recreating the object on every call (essentially the same behaviour as
    propertycache).

    '''
    def __init__(self, *paths):
        self.paths = paths

    def join(self, obj, fname):
        """Used to compute the runtime path of a cached file.

        Users should subclass filecache and provide their own version of this
        function to call the appropriate join function on 'obj' (an instance
        of the class that its member function was decorated).
        """
        return obj.join(fname)

    def __call__(self, func):
        self.func = func
        self.name = func.__name__
        return self

    def __get__(self, obj, type=None):
        # do we need to check if the file changed?
        if self.name in obj.__dict__:
            assert self.name in obj._filecache, self.name
            return obj.__dict__[self.name]

        entry = obj._filecache.get(self.name)

        if entry:
            if entry.changed():
                entry.obj = self.func(obj)
        else:
            paths = [self.join(obj, path) for path in self.paths]

            # We stat -before- creating the object so our cache doesn't lie if
            # a writer modified between the time we read and stat
            entry = filecacheentry(paths, True)
            entry.obj = self.func(obj)

            obj._filecache[self.name] = entry

        obj.__dict__[self.name] = entry.obj
        return entry.obj

    def __set__(self, obj, value):
        if self.name not in obj._filecache:
            # we add an entry for the missing value because X in __dict__
            # implies X in _filecache
            paths = [self.join(obj, path) for path in self.paths]
            ce = filecacheentry(paths, False)
            obj._filecache[self.name] = ce
        else:
            ce = obj._filecache[self.name]

        ce.obj = value # update cached copy
        obj.__dict__[self.name] = value # update copy returned by obj.x

    def __delete__(self, obj):
        try:
            del obj.__dict__[self.name]
        except KeyError:
            raise AttributeError(self.name)
