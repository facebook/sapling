# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util, error, osutil, revset, similar, encoding
import match as matchmod
import os, errno, re, stat, sys, glob

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
    def __init__(self, ui, abort, existingiter):
        self._ui = ui
        self._abort = abort
        self._map = {}
        for f in existingiter:
            self._map[encoding.lower(f)] = f

    def __call__(self, f):
        fl = encoding.lower(f)
        map = self._map
        if fl in map and map[fl] != f:
            msg = _('possible case-folding collision for %s') % f
            if self._abort:
                raise util.Abort(msg)
            self._ui.warn(_("warning: %s\n") % msg)
        map[fl] = f

class pathauditor(object):
    '''ensure that a filesystem path contains no banned components.
    the following properties of a path are checked:

    - ends with a directory separator
    - under top-level .hg
    - starts at the root of a windows drive
    - contains ".."
    - traverses a symlink (e.g. a/symlink_here/b)
    - inside a nested repository (a callback can be used to approve
      some nested repositories, e.g., subrepositories)
    '''

    def __init__(self, root, callback=None):
        self.audited = set()
        self.auditeddir = set()
        self.root = root
        self.callback = callback

    def __call__(self, path):
        '''Check the relative path.
        path may contain a pattern (e.g. foodir/**.txt)'''

        if path in self.audited:
            return
        # AIX ignores "/" at end of path, others raise EISDIR.
        if util.endswithsep(path):
            raise util.Abort(_("path ends in directory separator: %s") % path)
        normpath = os.path.normcase(path)
        parts = util.splitpath(normpath)
        if (os.path.splitdrive(path)[0]
            or parts[0].lower() in ('.hg', '.hg.', '')
            or os.pardir in parts):
            raise util.Abort(_("path contains illegal component: %s") % path)
        if '.hg' in path.lower():
            lparts = [p.lower() for p in parts]
            for p in '.hg', '.hg.':
                if p in lparts[1:]:
                    pos = lparts.index(p)
                    base = os.path.join(*parts[:pos])
                    raise util.Abort(_('path %r is inside nested repo %r')
                                     % (path, base))

        parts.pop()
        prefixes = []
        while parts:
            prefix = os.sep.join(parts)
            if prefix in self.auditeddir:
                break
            curpath = os.path.join(self.root, prefix)
            try:
                st = os.lstat(curpath)
            except OSError, err:
                # EINVAL can be raised as invalid path syntax under win32.
                # They must be ignored for patterns can be checked too.
                if err.errno not in (errno.ENOENT, errno.ENOTDIR, errno.EINVAL):
                    raise
            else:
                if stat.S_ISLNK(st.st_mode):
                    raise util.Abort(
                        _('path %r traverses symbolic link %r')
                        % (path, prefix))
                elif (stat.S_ISDIR(st.st_mode) and
                      os.path.isdir(os.path.join(curpath, '.hg'))):
                    if not self.callback or not self.callback(curpath):
                        raise util.Abort(_('path %r is inside nested repo %r') %
                                         (path, prefix))
            prefixes.append(prefix)
            parts.pop()

        self.audited.add(path)
        # only add prefixes to the cache after checking everything: we don't
        # want to add "foo/bar/baz" before checking if there's a "foo/.hg"
        self.auditeddir.update(prefixes)

class abstractopener(object):
    """Abstract base class; cannot be instantiated"""

    def __init__(self, *args, **kwargs):
        '''Prevent instantiation; don't call this from subclasses.'''
        raise NotImplementedError('attempted instantiating ' + str(type(self)))

    def read(self, path):
        fp = self(path, 'rb')
        try:
            return fp.read()
        finally:
            fp.close()

    def write(self, path, data):
        fp = self(path, 'wb')
        try:
            return fp.write(data)
        finally:
            fp.close()

    def append(self, path, data):
        fp = self(path, 'ab')
        try:
            return fp.write(data)
        finally:
            fp.close()

class opener(abstractopener):
    '''Open files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.
    '''
    def __init__(self, base, audit=True):
        self.base = base
        self._audit = audit
        if audit:
            self.auditor = pathauditor(base)
        else:
            self.auditor = util.always
        self.createmode = None
        self._trustnlink = None

    @util.propertycache
    def _cansymlink(self):
        return util.checklink(self.base)

    def _fixfilemode(self, name):
        if self.createmode is None:
            return
        os.chmod(name, self.createmode & 0666)

    def __call__(self, path, mode="r", text=False, atomictemp=False):
        if self._audit:
            r = util.checkosfilename(path)
            if r:
                raise util.Abort("%s: %r" % (r, path))
        self.auditor(path)
        f = os.path.join(self.base, path)

        if not text and "b" not in mode:
            mode += "b" # for that other OS

        nlink = -1
        dirname, basename = os.path.split(f)
        # If basename is empty, then the path is malformed because it points
        # to a directory. Let the posixfile() call below raise IOError.
        if basename and mode not in ('r', 'rb'):
            if atomictemp:
                if not os.path.isdir(dirname):
                    util.makedirs(dirname, self.createmode)
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
                if not os.path.isdir(dirname):
                    util.makedirs(dirname, self.createmode)
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
        self.auditor(dst)
        linkname = os.path.join(self.base, dst)
        try:
            os.unlink(linkname)
        except OSError:
            pass

        dirname = os.path.dirname(linkname)
        if not os.path.exists(dirname):
            util.makedirs(dirname, self.createmode)

        if self._cansymlink:
            try:
                os.symlink(src, linkname)
            except OSError, err:
                raise OSError(err.errno, _('could not symlink to %r: %s') %
                              (src, err.strerror), linkname)
        else:
            f = self(dst, "w")
            f.write(src)
            f.close()
            self._fixfilemode(dst)

    def audit(self, path):
        self.auditor(path)

class filteropener(abstractopener):
    '''Wrapper opener for filtering filenames with a function.'''

    def __init__(self, opener, filter):
        self._filter = filter
        self._orig = opener

    def __call__(self, path, *args, **kwargs):
        return self._orig(self._filter(path), *args, **kwargs)

def canonpath(root, cwd, myname, auditor=None):
    '''return the canonical path of myname, given cwd and root'''
    if util.endswithsep(root):
        rootsep = root
    else:
        rootsep = root + os.sep
    name = myname
    if not os.path.isabs(name):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if auditor is None:
        auditor = pathauditor(root)
    if name != rootsep and name.startswith(rootsep):
        name = name[len(rootsep):]
        auditor(name)
        return util.pconvert(name)
    elif name == root:
        return ''
    else:
        # Determine whether `name' is in the hierarchy at or beneath `root',
        # by iterating name=dirname(name) until that causes no change (can't
        # check name == '/', because that doesn't work on windows).  For each
        # `name', compare dev/inode numbers.  If they match, the list `rel'
        # holds the reversed list of components making up the relative file
        # name we want.
        root_st = os.stat(root)
        rel = []
        while True:
            try:
                name_st = os.stat(name)
            except OSError:
                break
            if util.samestat(name_st, root_st):
                if not rel:
                    # name was actually the same as root (maybe a symlink)
                    return ''
                rel.reverse()
                name = os.path.join(*rel)
                auditor(name)
                return util.pconvert(name)
            dirname, basename = os.path.split(name)
            rel.append(basename)
            if dirname == name:
                break
            name = dirname

        raise util.Abort('%s not under root' % myname)

def walkrepos(path, followsym=False, seen_dirs=None, recurse=False):
    '''yield every hg repository under path, recursively.'''
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
    path = systemrcpath()
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

if os.name != 'nt':

    def rcfiles(path):
        rcs = [os.path.join(path, 'hgrc')]
        rcdir = os.path.join(path, 'hgrc.d')
        try:
            rcs.extend([os.path.join(rcdir, f)
                        for f, kind in osutil.listdir(rcdir)
                        if f.endswith(".rc")])
        except OSError:
            pass
        return rcs

    def systemrcpath():
        path = []
        # old mod_python does not set sys.argv
        if len(getattr(sys, 'argv', [])) > 0:
            p = os.path.dirname(os.path.dirname(sys.argv[0]))
            path.extend(rcfiles(os.path.join(p, 'etc/mercurial')))
        path.extend(rcfiles('/etc/mercurial'))
        return path

    def userrcpath():
        return [os.path.expanduser('~/.hgrc')]

else:

    _HKEY_LOCAL_MACHINE = 0x80000002L

    def systemrcpath():
        '''return default os-specific hgrc search path'''
        rcpath = []
        filename = util.executablepath()
        # Use mercurial.ini found in directory with hg.exe
        progrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
        if os.path.isfile(progrc):
            rcpath.append(progrc)
            return rcpath
        # Use hgrc.d found in directory with hg.exe
        progrcd = os.path.join(os.path.dirname(filename), 'hgrc.d')
        if os.path.isdir(progrcd):
            for f, kind in osutil.listdir(progrcd):
                if f.endswith('.rc'):
                    rcpath.append(os.path.join(progrcd, f))
            return rcpath
        # else look for a system rcpath in the registry
        value = util.lookupreg('SOFTWARE\\Mercurial', None,
                               _HKEY_LOCAL_MACHINE)
        if not isinstance(value, str) or not value:
            return rcpath
        value = value.replace('/', os.sep)
        for p in value.split(os.pathsep):
            if p.lower().endswith('mercurial.ini'):
                rcpath.append(p)
            elif os.path.isdir(p):
                for f, kind in osutil.listdir(p):
                    if f.endswith('.rc'):
                        rcpath.append(os.path.join(p, f))
        return rcpath

    def userrcpath():
        '''return os-specific hgrc search path to the user dir'''
        home = os.path.expanduser('~')
        path = [os.path.join(home, 'mercurial.ini'),
                os.path.join(home, '.hgrc')]
        userprofile = os.environ.get('USERPROFILE')
        if userprofile:
            path.append(os.path.join(userprofile, 'mercurial.ini'))
            path.append(os.path.join(userprofile, '.hgrc'))
        return path

def revsingle(repo, revspec, default='.'):
    if not revspec:
        return repo[default]

    l = revrange(repo, [revspec])
    if len(l) < 1:
        raise util.Abort(_('empty revision set'))
    return repo[l[-1]]

def revpair(repo, revs):
    if not revs:
        return repo.dirstate.p1(), None

    l = revrange(repo, revs)

    if len(l) == 0:
        return repo.dirstate.p1(), None

    if len(l) == 1:
        return repo.lookup(l[0]), None

    return repo.lookup(l[0]), repo.lookup(l[-1])

_revrangesep = ':'

def revrange(repo, revs):
    """Yield revision as strings from a list of revision specifications."""

    def revfix(repo, val, defval):
        if not val and val != 0 and defval is not None:
            return defval
        return repo.changelog.rev(repo.lookup(val))

    seen, l = set(), []
    for spec in revs:
        # attempt to parse old-style ranges first to deal with
        # things like old-tag which contain query metacharacters
        try:
            if isinstance(spec, int):
                seen.add(spec)
                l.append(spec)
                continue

            if _revrangesep in spec:
                start, end = spec.split(_revrangesep, 1)
                start = revfix(repo, start, 0)
                end = revfix(repo, end, len(repo) - 1)
                step = start > end and -1 or 1
                for rev in xrange(start, end + step, step):
                    if rev in seen:
                        continue
                    seen.add(rev)
                    l.append(rev)
                continue
            elif spec and spec in repo: # single unquoted rev
                rev = revfix(repo, spec, None)
                if rev in seen:
                    continue
                seen.add(rev)
                l.append(rev)
                continue
        except error.RepoLookupError:
            pass

        # fall through to new-style queries if old-style fails
        m = revset.match(repo.ui, spec)
        for r in m(repo, range(len(repo))):
            if r not in seen:
                l.append(r)
        seen.update(l)

    return l

def expandpats(pats):
    if not util.expandglobs:
        return list(pats)
    ret = []
    for p in pats:
        kind, name = matchmod._patsplit(p, None)
        if kind is None:
            try:
                globbed = glob.glob(name)
            except re.error:
                globbed = [name]
            if globbed:
                ret.extend(globbed)
                continue
        ret.append(p)
    return ret

def match(ctx, pats=[], opts={}, globbed=False, default='relpath'):
    if pats == ("",):
        pats = []
    if not globbed and default == 'relpath':
        pats = expandpats(pats or [])

    m = ctx.match(pats, opts.get('include'), opts.get('exclude'),
                         default)
    def badfn(f, msg):
        ctx._repo.ui.warn("%s: %s\n" % (m.rel(f), msg))
    m.bad = badfn
    return m

def matchall(repo):
    return matchmod.always(repo.root, repo.getcwd())

def matchfiles(repo, files):
    return matchmod.exact(repo.root, repo.getcwd(), files)

def addremove(repo, pats=[], opts={}, dry_run=None, similarity=None):
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)
    # we'd use status here, except handling of symlinks and ignore is tricky
    added, unknown, deleted, removed = [], [], [], []
    audit_path = pathauditor(repo.root)
    m = match(repo[None], pats, opts)
    for abs in repo.walk(m):
        target = repo.wjoin(abs)
        good = True
        try:
            audit_path(abs)
        except (OSError, util.Abort):
            good = False
        rel = m.rel(abs)
        exact = m.exact(abs)
        if good and abs not in repo.dirstate:
            unknown.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('adding %s\n') % ((pats and rel) or abs))
        elif repo.dirstate[abs] != 'r' and (not good or not os.path.lexists(target)
            or (os.path.isdir(target) and not os.path.islink(target))):
            deleted.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('removing %s\n') % ((pats and rel) or abs))
        # for finding renames
        elif repo.dirstate[abs] == 'r':
            removed.append(abs)
        elif repo.dirstate[abs] == 'a':
            added.append(abs)
    copies = {}
    if similarity > 0:
        for old, new, score in similar.findrenames(repo,
                added + unknown, removed + deleted, similarity):
            if repo.ui.verbose or not m.exact(old) or not m.exact(new):
                repo.ui.status(_('recording removal of %s as rename to %s '
                                 '(%d%% similar)\n') %
                               (m.rel(old), m.rel(new), score * 100))
            copies[new] = old

    if not dry_run:
        wctx = repo[None]
        wlock = repo.wlock()
        try:
            wctx.forget(deleted)
            wctx.add(unknown)
            for new, old in copies.iteritems():
                wctx.copy(old, new)
        finally:
            wlock.release()

def updatedir(ui, repo, patches, similarity=0):
    '''Update dirstate after patch application according to metadata'''
    if not patches:
        return []
    copies = []
    removes = set()
    cfiles = patches.keys()
    cwd = repo.getcwd()
    if cwd:
        cfiles = [util.pathto(repo.root, cwd, f) for f in patches.keys()]
    for f in patches:
        gp = patches[f]
        if not gp:
            continue
        if gp.op == 'RENAME':
            copies.append((gp.oldpath, gp.path))
            removes.add(gp.oldpath)
        elif gp.op == 'COPY':
            copies.append((gp.oldpath, gp.path))
        elif gp.op == 'DELETE':
            removes.add(gp.path)

    wctx = repo[None]
    for src, dst in copies:
        dirstatecopy(ui, repo, wctx, src, dst, cwd=cwd)
    if (not similarity) and removes:
        wctx.remove(sorted(removes), True)

    for f in patches:
        gp = patches[f]
        if gp and gp.mode:
            islink, isexec = gp.mode
            dst = repo.wjoin(gp.path)
            # patch won't create empty files
            if gp.op == 'ADD' and not os.path.lexists(dst):
                flags = (isexec and 'x' or '') + (islink and 'l' or '')
                repo.wwrite(gp.path, '', flags)
            util.setflags(dst, islink, isexec)
    addremove(repo, cfiles, similarity=similarity)
    files = patches.keys()
    files.extend([r for r in removes if r not in files])
    return sorted(files)

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
        raise error.RequirementError(_("unknown repository format: "
            "requires features '%s' (upgrade Mercurial)") % "', '".join(missings))
    return requirements

class filecacheentry(object):
    def __init__(self, path):
        self.path = path
        self.cachestat = filecacheentry.stat(self.path)

        if self.cachestat:
            self._cacheable = self.cachestat.cacheable()
        else:
            # None means we don't know yet
            self._cacheable = None

    def refresh(self):
        if self.cacheable():
            self.cachestat = filecacheentry.stat(self.path)

    def cacheable(self):
        if self._cacheable is not None:
            return self._cacheable

        # we don't know yet, assume it is for now
        return True

    def changed(self):
        # no point in going further if we can't cache it
        if not self.cacheable():
            return True

        newstat = filecacheentry.stat(self.path)

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

class filecache(object):
    '''A property like decorator that tracks a file under .hg/ for updates.

    Records stat info when called in _filecache.

    On subsequent calls, compares old stat info with new info, and recreates
    the object when needed, updating the new stat info in _filecache.

    Mercurial either atomic renames or appends for files under .hg,
    so to ensure the cache is reliable we need the filesystem to be able
    to tell us if a file has been replaced. If it can't, we fallback to
    recreating the object on every call (essentially the same behaviour as
    propertycache).'''
    def __init__(self, path, instore=False):
        self.path = path
        self.instore = instore

    def __call__(self, func):
        self.func = func
        self.name = func.__name__
        return self

    def __get__(self, obj, type=None):
        entry = obj._filecache.get(self.name)

        if entry:
            if entry.changed():
                entry.obj = self.func(obj)
        else:
            path = self.instore and obj.sjoin(self.path) or obj.join(self.path)

            # We stat -before- creating the object so our cache doesn't lie if
            # a writer modified between the time we read and stat
            entry = filecacheentry(path)
            entry.obj = self.func(obj)

            obj._filecache[self.name] = entry

        setattr(obj, self.name, entry.obj)
        return entry.obj
