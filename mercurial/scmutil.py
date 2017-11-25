# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import glob
import hashlib
import os
import re
import socket
import subprocess
import weakref

from .i18n import _
from .node import (
    hex,
    nullid,
    short,
    wdirid,
    wdirrev,
)

from . import (
    encoding,
    error,
    match as matchmod,
    obsolete,
    obsutil,
    pathutil,
    phases,
    pycompat,
    revsetlang,
    similar,
    url,
    util,
    vfs,
)

if pycompat.iswindows:
    from . import scmwindows as scmplatform
else:
    from . import scmposix as scmplatform

termsize = scmplatform.termsize

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
            ctx = repo[n]
            if ctx.phase() >= phases.secret and not ctx.extinct():
                secretlist.append(n)

    if secretlist:
        ui.status(_("no changes found (ignored %d secret changesets)\n")
                  % len(secretlist))
    else:
        ui.status(_("no changes found\n"))

def callcatch(ui, func):
    """call func() with global exception handling

    return func() if no exception happens. otherwise do some error handling
    and return an exit code accordingly. does not handle all exceptions.
    """
    try:
        try:
            return func()
        except: # re-raises
            ui.traceback()
            raise
    # Global exception handling, alphabetically
    # Mercurial-specific first, followed by built-in and library exceptions
    except error.LockHeld as inst:
        if inst.errno == errno.ETIMEDOUT:
            reason = _('timed out waiting for lock held by %r') % inst.locker
        else:
            reason = _('lock held by %r') % inst.locker
        ui.warn(_("abort: %s: %s\n") % (inst.desc or inst.filename, reason))
        if not inst.locker:
            ui.warn(_("(lock might be very busy)\n"))
    except error.LockUnavailable as inst:
        ui.warn(_("abort: could not lock %s: %s\n") %
               (inst.desc or inst.filename,
                encoding.strtolocal(inst.strerror)))
    except error.OutOfBandError as inst:
        if inst.args:
            msg = _("abort: remote error:\n")
        else:
            msg = _("abort: remote error\n")
        ui.warn(msg)
        if inst.args:
            ui.warn(''.join(inst.args))
        if inst.hint:
            ui.warn('(%s)\n' % inst.hint)
    except error.RepoError as inst:
        ui.warn(_("abort: %s!\n") % inst)
        if inst.hint:
            ui.warn(_("(%s)\n") % inst.hint)
    except error.ResponseError as inst:
        ui.warn(_("abort: %s") % inst.args[0])
        if not isinstance(inst.args[1], basestring):
            ui.warn(" %r\n" % (inst.args[1],))
        elif not inst.args[1]:
            ui.warn(_(" empty string\n"))
        else:
            ui.warn("\n%r\n" % util.ellipsis(inst.args[1]))
    except error.CensoredNodeError as inst:
        ui.warn(_("abort: file censored %s!\n") % inst)
    except error.RevlogError as inst:
        ui.warn(_("abort: %s!\n") % inst)
    except error.InterventionRequired as inst:
        ui.warn("%s\n" % inst)
        if inst.hint:
            ui.warn(_("(%s)\n") % inst.hint)
        return 1
    except error.WdirUnsupported:
        ui.warn(_("abort: working directory revision cannot be specified\n"))
    except error.Abort as inst:
        ui.warn(_("abort: %s\n") % inst)
        if inst.hint:
            ui.warn(_("(%s)\n") % inst.hint)
    except ImportError as inst:
        ui.warn(_("abort: %s!\n") % inst)
        m = str(inst).split()[-1]
        if m in "mpatch bdiff".split():
            ui.warn(_("(did you forget to compile extensions?)\n"))
        elif m in "zlib".split():
            ui.warn(_("(is your Python install correct?)\n"))
    except IOError as inst:
        if util.safehasattr(inst, "code"):
            ui.warn(_("abort: %s\n") % inst)
        elif util.safehasattr(inst, "reason"):
            try: # usually it is in the form (errno, strerror)
                reason = inst.reason.args[1]
            except (AttributeError, IndexError):
                # it might be anything, for example a string
                reason = inst.reason
            if isinstance(reason, unicode):
                # SSLError of Python 2.7.9 contains a unicode
                reason = encoding.unitolocal(reason)
            ui.warn(_("abort: error: %s\n") % reason)
        elif (util.safehasattr(inst, "args")
              and inst.args and inst.args[0] == errno.EPIPE):
            pass
        elif getattr(inst, "strerror", None):
            if getattr(inst, "filename", None):
                ui.warn(_("abort: %s: %s\n") % (
                    encoding.strtolocal(inst.strerror), inst.filename))
            else:
                ui.warn(_("abort: %s\n") % encoding.strtolocal(inst.strerror))
        else:
            raise
    except OSError as inst:
        if getattr(inst, "filename", None) is not None:
            ui.warn(_("abort: %s: '%s'\n") % (
                encoding.strtolocal(inst.strerror), inst.filename))
        else:
            ui.warn(_("abort: %s\n") % encoding.strtolocal(inst.strerror))
    except MemoryError:
        ui.warn(_("abort: out of memory\n"))
    except SystemExit as inst:
        # Commands shouldn't sys.exit directly, but give a return code.
        # Just in case catch this and and pass exit code to caller.
        return inst.code
    except socket.error as inst:
        ui.warn(_("abort: %s\n") % inst.args[-1])

    return -1

def checknewlabel(repo, lbl, kind):
    # Do not use the "kind" parameter in ui output.
    # It makes strings difficult to translate.
    if lbl in ['tip', '.', 'null']:
        raise error.Abort(_("the name '%s' is reserved") % lbl)
    for c in (':', '\0', '\n', '\r'):
        if c in lbl:
            raise error.Abort(_("%r cannot be used in a name") % c)
    try:
        int(lbl)
        raise error.Abort(_("cannot use an integer as a name"))
    except ValueError:
        pass

def checkfilename(f):
    '''Check that the filename f is an acceptable filename for a tracked file'''
    if '\r' in f or '\n' in f:
        raise error.Abort(_("'\\n' and '\\r' disallowed in filenames: %r") % f)

def checkportable(ui, f):
    '''Check if filename f is portable and warn or abort depending on config'''
    checkfilename(f)
    abort, warn = checkportabilityalert(ui)
    if abort or warn:
        msg = util.checkwinfilename(f)
        if msg:
            msg = "%s: %s" % (msg, util.shellquote(f))
            if abort:
                raise error.Abort(msg)
            ui.warn(_("warning: %s\n") % msg)

def checkportabilityalert(ui):
    '''check if the user's config requests nothing, a warning, or abort for
    non-portable filenames'''
    val = ui.config('ui', 'portablefilenames')
    lval = val.lower()
    bval = util.parsebool(val)
    abort = pycompat.iswindows or lval == 'abort'
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
                raise error.Abort(msg)
            self._ui.warn(_("warning: %s\n") % msg)
        self._loweredfiles.add(fl)
        self._newfiles.add(f)

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
        s = hashlib.sha1()
        for rev in revs:
            s.update('%d;' % rev)
        key = s.digest()
    return key

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

def binnode(ctx):
    """Return binary node id for a given basectx"""
    node = ctx.node()
    if node is None:
        return wdirid
    return node

def intrev(ctx):
    """Return integer for a given basectx that can be used in comparison or
    arithmetic operation"""
    rev = ctx.rev()
    if rev is None:
        return wdirrev
    return rev

def formatchangeid(ctx):
    """Format changectx as '{rev}:{node|formatnode}', which is the default
    template provided by cmdutil.changeset_templater"""
    repo = ctx.repo()
    return formatrevnode(repo.ui, intrev(ctx), binnode(ctx))

def formatrevnode(ui, rev, node):
    """Format given revision and node depending on the current verbosity"""
    if ui.debugflag:
        hexfunc = hex
    else:
        hexfunc = short
    return '%d:%s' % (rev, hexfunc(node))

def revsingle(repo, revspec, default='.', localalias=None):
    if not revspec and revspec != 0:
        return repo[default]

    l = revrange(repo, [revspec], localalias=localalias)
    if not l:
        raise error.Abort(_('empty revision set'))
    return repo[l.last()]

def _pairspec(revspec):
    tree = revsetlang.parse(revspec)
    return tree and tree[0] in ('range', 'rangepre', 'rangepost', 'rangeall')

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
        raise error.Abort(_('empty revision range'))
    if (first == second and len(revs) >= 2
        and not all(revrange(repo, [r]) for r in revs)):
        raise error.Abort(_('empty revision on one side of range'))

    # if top-level is range expression, the result must always be a pair
    if first == second and len(revs) == 1 and not _pairspec(revs[0]):
        return repo.lookup(first), None

    return repo.lookup(first), repo.lookup(second)

def revrange(repo, specs, localalias=None):
    """Execute 1 to many revsets and return the union.

    This is the preferred mechanism for executing revsets using user-specified
    config options, such as revset aliases.

    The revsets specified by ``specs`` will be executed via a chained ``OR``
    expression. If ``specs`` is empty, an empty result is returned.

    ``specs`` can contain integers, in which case they are assumed to be
    revision numbers.

    It is assumed the revsets are already formatted. If you have arguments
    that need to be expanded in the revset, call ``revsetlang.formatspec()``
    and pass the result as an element of ``specs``.

    Specifying a single revset is allowed.

    Returns a ``revset.abstractsmartset`` which is a list-like interface over
    integer revisions.
    """
    allspecs = []
    for spec in specs:
        if isinstance(spec, int):
            spec = revsetlang.formatspec('rev(%d)', spec)
        allspecs.append(spec)
    return repo.anyrevs(allspecs, user=True, localalias=localalias)

def meaningfulparents(repo, ctx):
    """Return list of meaningful (or all if debug) parentrevs for rev.

    For merges (two non-nullrev revisions) both parents are meaningful.
    Otherwise the first parent revision is considered meaningful if it
    is not the preceding revision.
    """
    parents = ctx.parents()
    if len(parents) > 1:
        return parents
    if repo.ui.debugflag:
        return [parents[0], repo['null']]
    if parents[0].rev() >= intrev(ctx) - 1:
        return []
    return parents

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

def matchandpats(ctx, pats=(), opts=None, globbed=False, default='relpath',
                 badfn=None):
    '''Return a matcher and the patterns that were used.
    The matcher will warn about bad matches, unless an alternate badfn callback
    is provided.'''
    if pats == ("",):
        pats = []
    if opts is None:
        opts = {}
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

def match(ctx, pats=(), opts=None, globbed=False, default='relpath',
          badfn=None):
    '''Return a matcher that will warn about bad matches.'''
    return matchandpats(ctx, pats, opts, globbed, default, badfn=badfn)[0]

def matchall(repo):
    '''Return a matcher that will efficiently match everything.'''
    return matchmod.always(repo.root, repo.getcwd())

def matchfiles(repo, files, badfn=None):
    '''Return a matcher that will efficiently match exactly these files.'''
    return matchmod.exact(repo.root, repo.getcwd(), files, badfn=badfn)

def parsefollowlinespattern(repo, rev, pat, msg):
    """Return a file name from `pat` pattern suitable for usage in followlines
    logic.
    """
    if not matchmod.patkind(pat):
        return pathutil.canonpath(repo.root, repo.getcwd(), pat)
    else:
        ctx = repo[rev]
        m = matchmod.match(repo.root, repo.getcwd(), [pat], ctx=ctx)
        files = [f for f in ctx if m(f)]
        if len(files) != 1:
            raise error.ParseError(msg)
        return files[0]

def origpath(ui, repo, filepath):
    '''customize where .orig files are created

    Fetch user defined path from config file: [ui] origbackuppath = <path>
    Fall back to default (filepath with .orig suffix) if not specified
    '''
    origbackuppath = ui.config('ui', 'origbackuppath')
    if not origbackuppath:
        return filepath + ".orig"

    # Convert filepath from an absolute path into a path inside the repo.
    filepathfromroot = util.normpath(os.path.relpath(filepath,
                                                     start=repo.root))

    origvfs = vfs.vfs(repo.wjoin(origbackuppath))
    origbackupdir = origvfs.dirname(filepathfromroot)
    if not origvfs.isdir(origbackupdir) or origvfs.islink(origbackupdir):
        ui.note(_('creating directory: %s\n') % origvfs.join(origbackupdir))

        # Remove any files that conflict with the backup file's path
        for f in reversed(list(util.finddirs(filepathfromroot))):
            if origvfs.isfileorlink(f):
                ui.note(_('removing conflicting file: %s\n')
                        % origvfs.join(f))
                origvfs.unlink(f)
                break

        origvfs.makedirs(origbackupdir)

    if origvfs.isdir(filepathfromroot) and not origvfs.islink(filepathfromroot):
        ui.note(_('removing conflicting directory: %s\n')
                % origvfs.join(filepathfromroot))
        origvfs.rmtree(filepathfromroot, forcibly=True)

    return origvfs.join(filepathfromroot)

class _containsnode(object):
    """proxy __contains__(node) to container.__contains__ which accepts revs"""

    def __init__(self, repo, revcontainer):
        self._torev = repo.changelog.rev
        self._revcontains = revcontainer.__contains__

    def __contains__(self, node):
        return self._revcontains(self._torev(node))

def cleanupnodes(repo, replacements, operation, moves=None, metadata=None):
    """do common cleanups when old nodes are replaced by new nodes

    That includes writing obsmarkers or stripping nodes, and moving bookmarks.
    (we might also want to move working directory parent in the future)

    By default, bookmark moves are calculated automatically from 'replacements',
    but 'moves' can be used to override that. Also, 'moves' may include
    additional bookmark moves that should not have associated obsmarkers.

    replacements is {oldnode: [newnode]} or a iterable of nodes if they do not
    have replacements. operation is a string, like "rebase".

    metadata is dictionary containing metadata to be stored in obsmarker if
    obsolescence is enabled.
    """
    if not replacements and not moves:
        return

    # translate mapping's other forms
    if not util.safehasattr(replacements, 'items'):
        replacements = {n: () for n in replacements}

    # Calculate bookmark movements
    if moves is None:
        moves = {}
    # Unfiltered repo is needed since nodes in replacements might be hidden.
    unfi = repo.unfiltered()
    for oldnode, newnodes in replacements.items():
        if oldnode in moves:
            continue
        if len(newnodes) > 1:
            # usually a split, take the one with biggest rev number
            newnode = next(unfi.set('max(%ln)', newnodes)).node()
        elif len(newnodes) == 0:
            # move bookmark backwards
            roots = list(unfi.set('max((::%n) - %ln)', oldnode,
                                  list(replacements)))
            if roots:
                newnode = roots[0].node()
            else:
                newnode = nullid
        else:
            newnode = newnodes[0]
        moves[oldnode] = newnode

    with repo.transaction('cleanup') as tr:
        # Move bookmarks
        bmarks = repo._bookmarks
        bmarkchanges = []
        allnewnodes = [n for ns in replacements.values() for n in ns]
        for oldnode, newnode in moves.items():
            oldbmarks = repo.nodebookmarks(oldnode)
            if not oldbmarks:
                continue
            from . import bookmarks # avoid import cycle
            repo.ui.debug('moving bookmarks %r from %s to %s\n' %
                          (oldbmarks, hex(oldnode), hex(newnode)))
            # Delete divergent bookmarks being parents of related newnodes
            deleterevs = repo.revs('parents(roots(%ln & (::%n))) - parents(%n)',
                                   allnewnodes, newnode, oldnode)
            deletenodes = _containsnode(repo, deleterevs)
            for name in oldbmarks:
                bmarkchanges.append((name, newnode))
                for b in bookmarks.divergent2delete(repo, deletenodes, name):
                    bmarkchanges.append((b, None))

        if bmarkchanges:
            bmarks.applychanges(repo, tr, bmarkchanges)

        # Obsolete or strip nodes
        if obsolete.isenabled(repo, obsolete.createmarkersopt):
            # If a node is already obsoleted, and we want to obsolete it
            # without a successor, skip that obssolete request since it's
            # unnecessary. That's the "if s or not isobs(n)" check below.
            # Also sort the node in topology order, that might be useful for
            # some obsstore logic.
            # NOTE: the filtering and sorting might belong to createmarkers.
            isobs = unfi.obsstore.successors.__contains__
            torev = unfi.changelog.rev
            sortfunc = lambda ns: torev(ns[0])
            rels = [(unfi[n], tuple(unfi[m] for m in s))
                    for n, s in sorted(replacements.items(), key=sortfunc)
                    if s or not isobs(n)]
            if rels:
                obsolete.createmarkers(repo, rels, operation=operation,
                                       metadata=metadata)
        else:
            from . import repair # avoid import cycle
            tostrip = list(replacements)
            if tostrip:
                repair.delayedstrip(repo.ui, repo, tostrip, operation)

def addremove(repo, matcher, prefix, opts=None, dry_run=None, similarity=None):
    if opts is None:
        opts = {}
    m = matcher
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)

    ret = 0
    join = lambda f: os.path.join(prefix, f)

    wctx = repo[None]
    for subpath in sorted(wctx.substate):
        submatch = matchmod.subdirmatcher(subpath, m)
        if opts.get('subrepos') or m.exact(subpath) or any(submatch.files()):
            sub = wctx.sub(subpath)
            try:
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
    audit_path = pathutil.pathauditor(repo.root, cached=True)

    ctx = repo[None]
    dirstate = repo.dirstate
    walkresults = dirstate.walk(matcher, subrepos=sorted(ctx.substate),
                                unknown=True, ignored=False, full=False)
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
    with repo.wlock():
        wctx.forget(deleted)
        wctx.add(unknown)
        for new, old in renames.iteritems():
            wctx.copy(old, new)

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
            hint=_("see https://mercurial-scm.org/wiki/MissingRequirement"
                   " for more information"))
    return requirements

def writerequires(opener, requirements):
    with opener('requires', 'w') as fp:
        for r in sorted(requirements):
            fp.write("%s\n" % r)

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
        except OSError as e:
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
    recreating the object on every call (essentially the same behavior as
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
        raise NotImplementedError

    def __call__(self, func):
        self.func = func
        self.name = func.__name__.encode('ascii')
        return self

    def __get__(self, obj, type=None):
        # if accessed on the class, return the descriptor itself.
        if obj is None:
            return self
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

def extdatasource(repo, source):
    """Gather a map of rev -> value dict from the specified source

    A source spec is treated as a URL, with a special case shell: type
    for parsing the output from a shell command.

    The data is parsed as a series of newline-separated records where
    each record is a revision specifier optionally followed by a space
    and a freeform string value. If the revision is known locally, it
    is converted to a rev, otherwise the record is skipped.

    Note that both key and value are treated as UTF-8 and converted to
    the local encoding. This allows uniformity between local and
    remote data sources.
    """

    spec = repo.ui.config("extdata", source)
    if not spec:
        raise error.Abort(_("unknown extdata source '%s'") % source)

    data = {}
    src = proc = None
    try:
        if spec.startswith("shell:"):
            # external commands should be run relative to the repo root
            cmd = spec[6:]
            proc = subprocess.Popen(cmd, shell=True, bufsize=-1,
                                    close_fds=util.closefds,
                                    stdout=subprocess.PIPE, cwd=repo.root)
            src = proc.stdout
        else:
            # treat as a URL or file
            src = url.open(repo.ui, spec)
        for l in src:
            if " " in l:
                k, v = l.strip().split(" ", 1)
            else:
                k, v = l.strip(), ""

            k = encoding.tolocal(k)
            try:
                data[repo[k].rev()] = encoding.tolocal(v)
            except (error.LookupError, error.RepoLookupError):
                pass # we ignore data for nodes that don't exist locally
    finally:
        if proc:
            proc.communicate()
            if proc.returncode != 0:
                # not an error so 'cmd | grep' can be empty
                repo.ui.debug("extdata command '%s' %s\n"
                              % (cmd, util.explainexit(proc.returncode)[0]))
        if src:
            src.close()

    return data

def _locksub(repo, lock, envvar, cmd, environ=None, *args, **kwargs):
    if lock is None:
        raise error.LockInheritanceContractViolation(
            'lock can only be inherited while held')
    if environ is None:
        environ = {}
    with lock.inherit() as locker:
        environ[envvar] = locker
        return repo.ui.system(cmd, environ=environ, *args, **kwargs)

def wlocksub(repo, cmd, *args, **kwargs):
    """run cmd as a subprocess that allows inheriting repo's wlock

    This can only be called while the wlock is held. This takes all the
    arguments that ui.system does, and returns the exit code of the
    subprocess."""
    return _locksub(repo, repo.currentwlock(), 'HG_WLOCK_LOCKER', cmd, *args,
                    **kwargs)

def gdinitconfig(ui):
    """helper function to know if a repo should be created as general delta
    """
    # experimental config: format.generaldelta
    return (ui.configbool('format', 'generaldelta')
            or ui.configbool('format', 'usegeneraldelta'))

def gddeltaconfig(ui):
    """helper function to know if incoming delta should be optimised
    """
    # experimental config: format.generaldelta
    return ui.configbool('format', 'generaldelta')

class simplekeyvaluefile(object):
    """A simple file with key=value lines

    Keys must be alphanumerics and start with a letter, values must not
    contain '\n' characters"""
    firstlinekey = '__firstline'

    def __init__(self, vfs, path, keys=None):
        self.vfs = vfs
        self.path = path

    def read(self, firstlinenonkeyval=False):
        """Read the contents of a simple key-value file

        'firstlinenonkeyval' indicates whether the first line of file should
        be treated as a key-value pair or reuturned fully under the
        __firstline key."""
        lines = self.vfs.readlines(self.path)
        d = {}
        if firstlinenonkeyval:
            if not lines:
                e = _("empty simplekeyvalue file")
                raise error.CorruptedState(e)
            # we don't want to include '\n' in the __firstline
            d[self.firstlinekey] = lines[0][:-1]
            del lines[0]

        try:
            # the 'if line.strip()' part prevents us from failing on empty
            # lines which only contain '\n' therefore are not skipped
            # by 'if line'
            updatedict = dict(line[:-1].split('=', 1) for line in lines
                                                      if line.strip())
            if self.firstlinekey in updatedict:
                e = _("%r can't be used as a key")
                raise error.CorruptedState(e % self.firstlinekey)
            d.update(updatedict)
        except ValueError as e:
            raise error.CorruptedState(str(e))
        return d

    def write(self, data, firstline=None):
        """Write key=>value mapping to a file
        data is a dict. Keys must be alphanumerical and start with a letter.
        Values must not contain newline characters.

        If 'firstline' is not None, it is written to file before
        everything else, as it is, not in a key=value form"""
        lines = []
        if firstline is not None:
            lines.append('%s\n' % firstline)

        for k, v in data.items():
            if k == self.firstlinekey:
                e = "key name '%s' is reserved" % self.firstlinekey
                raise error.ProgrammingError(e)
            if not k[0].isalpha():
                e = "keys must start with a letter in a key-value file"
                raise error.ProgrammingError(e)
            if not k.isalnum():
                e = "invalid key name in a simple key-value file"
                raise error.ProgrammingError(e)
            if '\n' in v:
                e = "invalid value in a simple key-value file"
                raise error.ProgrammingError(e)
            lines.append("%s=%s\n" % (k, v))
        with self.vfs(self.path, mode='wb', atomictemp=True) as fp:
            fp.write(''.join(lines))

_reportobsoletedsource = [
    'debugobsolete',
    'pull',
    'push',
    'serve',
    'unbundle',
]

_reportnewcssource = [
    'pull',
    'unbundle',
]

def registersummarycallback(repo, otr, txnname=''):
    """register a callback to issue a summary after the transaction is closed
    """
    def txmatch(sources):
        return any(txnname.startswith(source) for source in sources)

    categories = []

    def reportsummary(func):
        """decorator for report callbacks."""
        # The repoview life cycle is shorter than the one of the actual
        # underlying repository. So the filtered object can die before the
        # weakref is used leading to troubles. We keep a reference to the
        # unfiltered object and restore the filtering when retrieving the
        # repository through the weakref.
        filtername = repo.filtername
        reporef = weakref.ref(repo.unfiltered())
        def wrapped(tr):
            repo = reporef()
            if filtername:
                repo = repo.filtered(filtername)
            func(repo, tr)
        newcat = '%2i-txnreport' % len(categories)
        otr.addpostclose(newcat, wrapped)
        categories.append(newcat)
        return wrapped

    if txmatch(_reportobsoletedsource):
        @reportsummary
        def reportobsoleted(repo, tr):
            obsoleted = obsutil.getobsoleted(repo, tr)
            if obsoleted:
                repo.ui.status(_('obsoleted %i changesets\n')
                               % len(obsoleted))

    if txmatch(_reportnewcssource):
        @reportsummary
        def reportnewcs(repo, tr):
            """Report the range of new revisions pulled/unbundled."""
            newrevs = list(tr.changes.get('revs', set()))
            if not newrevs:
                return

            # Compute the bounds of new revisions' range, excluding obsoletes.
            unfi = repo.unfiltered()
            revs = unfi.revs('%ld and not obsolete()', newrevs)
            if not revs:
                # Got only obsoletes.
                return
            minrev, maxrev = repo[revs.min()], repo[revs.max()]

            if minrev == maxrev:
                revrange = minrev
            else:
                revrange = '%s:%s' % (minrev, maxrev)
            repo.ui.status(_('new changesets %s\n') % revrange)
