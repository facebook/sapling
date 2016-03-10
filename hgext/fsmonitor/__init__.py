# __init__.py - fsmonitor initialization and overrides
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''Faster status operations with the Watchman file monitor (EXPERIMENTAL)

Integrates the file-watching program Watchman with Mercurial to produce faster
status results.

On a particular Linux system, for a real-world repository with over 400,000
files hosted on ext4, vanilla `hg status` takes 1.3 seconds. On the same
system, with fsmonitor it takes about 0.3 seconds.

fsmonitor requires no configuration -- it will tell Watchman about your
repository as necessary. You'll need to install Watchman from
https://facebook.github.io/watchman/ and make sure it is in your PATH.

The following configuration options exist:

::

    [fsmonitor]
    mode = {off, on, paranoid}

When `mode = off`, fsmonitor will disable itself (similar to not loading the
extension at all). When `mode = on`, fsmonitor will be enabled (the default).
When `mode = paranoid`, fsmonitor will query both Watchman and the filesystem,
and ensure that the results are consistent.

::

    [fsmonitor]
    timeout = (float)

A value, in seconds, that determines how long fsmonitor will wait for Watchman
to return results. Defaults to `2.0`.

::

    [fsmonitor]
    blacklistusers = (list of userids)

A list of usernames for which fsmonitor will disable itself altogether.

::

    [fsmonitor]
    walk_on_invalidate = (boolean)

Whether or not to walk the whole repo ourselves when our cached state has been
invalidated, for example when Watchman has been restarted or .hgignore rules
have been changed. Walking the repo in that case can result in competing for
I/O with Watchman. For large repos it is recommended to set this value to
false. You may wish to set this to true if you have a very fast filesystem
that can outpace the IPC overhead of getting the result data for the full repo
from Watchman. Defaults to false.

fsmonitor is incompatible with the largefiles and eol extensions, and
will disable itself if any of those are active.

'''

# Platforms Supported
# ===================
#
# **Linux:** *Stable*. Watchman and fsmonitor are both known to work reliably,
#   even under severe loads.
#
# **Mac OS X:** *Stable*. The Mercurial test suite passes with fsmonitor
#   turned on, on case-insensitive HFS+. There has been a reasonable amount of
#   user testing under normal loads.
#
# **Solaris, BSD:** *Alpha*. watchman and fsmonitor are believed to work, but
#   very little testing has been done.
#
# **Windows:** *Alpha*. Not in a release version of watchman or fsmonitor yet.
#
# Known Issues
# ============
#
# * fsmonitor will disable itself if any of the following extensions are
#   enabled: largefiles, inotify, eol; or if the repository has subrepos.
# * fsmonitor will produce incorrect results if nested repos that are not
#   subrepos exist. *Workaround*: add nested repo paths to your `.hgignore`.
#
# The issues related to nested repos and subrepos are probably not fundamental
# ones. Patches to fix them are welcome.

from __future__ import absolute_import

import os
import stat
import sys

from mercurial import (
    context,
    extensions,
    localrepo,
    merge,
    pathutil,
    scmutil,
    util,
)
from mercurial import match as matchmod
from mercurial.i18n import _

from . import (
    state,
    watchmanclient,
)

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

# This extension is incompatible with the following blacklisted extensions
# and will disable itself when encountering one of these:
_blacklist = ['largefiles', 'eol']

def _handleunavailable(ui, state, ex):
    """Exception handler for Watchman interaction exceptions"""
    if isinstance(ex, watchmanclient.Unavailable):
        if ex.warn:
            ui.warn(str(ex) + '\n')
        if ex.invalidate:
            state.invalidate()
        ui.log('fsmonitor', 'Watchman unavailable: %s\n', ex.msg)
    else:
        ui.log('fsmonitor', 'Watchman exception: %s\n', ex)

def _hashignore(ignore):
    """Calculate hash for ignore patterns and filenames

    If this information changes between Mercurial invocations, we can't
    rely on Watchman information anymore and have to re-scan the working
    copy.

    """
    sha1 = util.sha1()
    if util.safehasattr(ignore, 'includepat'):
        sha1.update(ignore.includepat)
    sha1.update('\0\0')
    if util.safehasattr(ignore, 'excludepat'):
        sha1.update(ignore.excludepat)
    sha1.update('\0\0')
    if util.safehasattr(ignore, 'patternspat'):
        sha1.update(ignore.patternspat)
    sha1.update('\0\0')
    if util.safehasattr(ignore, '_files'):
        for f in ignore._files:
            sha1.update(f)
    sha1.update('\0')
    return sha1.hexdigest()

def overridewalk(orig, self, match, subrepos, unknown, ignored, full=True):
    '''Replacement for dirstate.walk, hooking into Watchman.

    Whenever full is False, ignored is False, and the Watchman client is
    available, use Watchman combined with saved state to possibly return only a
    subset of files.'''
    def bail():
        return orig(match, subrepos, unknown, ignored, full=True)

    if full or ignored or not self._watchmanclient.available():
        return bail()
    state = self._fsmonitorstate
    clock, ignorehash, notefiles = state.get()
    if not clock:
        if state.walk_on_invalidate:
            return bail()
        # Initial NULL clock value, see
        # https://facebook.github.io/watchman/docs/clockspec.html
        clock = 'c:0:0'
        notefiles = []

    def fwarn(f, msg):
        self._ui.warn('%s: %s\n' % (self.pathto(f), msg))
        return False

    def badtype(mode):
        kind = _('unknown')
        if stat.S_ISCHR(mode):
            kind = _('character device')
        elif stat.S_ISBLK(mode):
            kind = _('block device')
        elif stat.S_ISFIFO(mode):
            kind = _('fifo')
        elif stat.S_ISSOCK(mode):
            kind = _('socket')
        elif stat.S_ISDIR(mode):
            kind = _('directory')
        return _('unsupported file type (type is %s)') % kind

    ignore = self._ignore
    dirignore = self._dirignore
    if unknown:
        if _hashignore(ignore) != ignorehash and clock != 'c:0:0':
            # ignore list changed -- can't rely on Watchman state any more
            if state.walk_on_invalidate:
                return bail()
            notefiles = []
            clock = 'c:0:0'
    else:
        # always ignore
        ignore = util.always
        dirignore = util.always

    matchfn = match.matchfn
    matchalways = match.always()
    dmap = self._map
    nonnormalset = getattr(self, '_nonnormalset', None)

    copymap = self._copymap
    getkind = stat.S_IFMT
    dirkind = stat.S_IFDIR
    regkind = stat.S_IFREG
    lnkkind = stat.S_IFLNK
    join = self._join
    normcase = util.normcase
    fresh_instance = False

    exact = skipstep3 = False
    if matchfn == match.exact:  # match.exact
        exact = True
        dirignore = util.always  # skip step 2
    elif match.files() and not match.anypats():  # match.match, no patterns
        skipstep3 = True

    if not exact and self._checkcase:
        # note that even though we could receive directory entries, we're only
        # interested in checking if a file with the same name exists. So only
        # normalize files if possible.
        normalize = self._normalizefile
        skipstep3 = False
    else:
        normalize = None

    # step 1: find all explicit files
    results, work, dirsnotfound = self._walkexplicit(match, subrepos)

    skipstep3 = skipstep3 and not (work or dirsnotfound)
    work = [d for d in work if not dirignore(d[0])]

    if not work and (exact or skipstep3):
        for s in subrepos:
            del results[s]
        del results['.hg']
        return results

    # step 2: query Watchman
    try:
        # Use the user-configured timeout for the query.
        # Add a little slack over the top of the user query to allow for
        # overheads while transferring the data
        self._watchmanclient.settimeout(state.timeout + 0.1)
        result = self._watchmanclient.command('query', {
            'fields': ['mode', 'mtime', 'size', 'exists', 'name'],
            'since': clock,
            'expression': [
                'not', [
                    'anyof', ['dirname', '.hg'],
                    ['name', '.hg', 'wholename']
                ]
            ],
            'sync_timeout': int(state.timeout * 1000),
            'empty_on_fresh_instance': state.walk_on_invalidate,
        })
    except Exception as ex:
        _handleunavailable(self._ui, state, ex)
        self._watchmanclient.clearconnection()
        return bail()
    else:
        # We need to propagate the last observed clock up so that we
        # can use it for our next query
        state.setlastclock(result['clock'])
        if result['is_fresh_instance']:
            if state.walk_on_invalidate:
                state.invalidate()
                return bail()
            fresh_instance = True
            # Ignore any prior noteable files from the state info
            notefiles = []

    # for file paths which require normalization and we encounter a case
    # collision, we store our own foldmap
    if normalize:
        foldmap = dict((normcase(k), k) for k in results)

    switch_slashes = os.sep == '\\'
    # The order of the results is, strictly speaking, undefined.
    # For case changes on a case insensitive filesystem we may receive
    # two entries, one with exists=True and another with exists=False.
    # The exists=True entries in the same response should be interpreted
    # as being happens-after the exists=False entries due to the way that
    # Watchman tracks files.  We use this property to reconcile deletes
    # for name case changes.
    for entry in result['files']:
        fname = entry['name']
        if switch_slashes:
            fname = fname.replace('\\', '/')
        if normalize:
            normed = normcase(fname)
            fname = normalize(fname, True, True)
            foldmap[normed] = fname
        fmode = entry['mode']
        fexists = entry['exists']
        kind = getkind(fmode)

        if not fexists:
            # if marked as deleted and we don't already have a change
            # record, mark it as deleted.  If we already have an entry
            # for fname then it was either part of walkexplicit or was
            # an earlier result that was a case change
            if fname not in results and fname in dmap and (
                    matchalways or matchfn(fname)):
                results[fname] = None
        elif kind == dirkind:
            if fname in dmap and (matchalways or matchfn(fname)):
                results[fname] = None
        elif kind == regkind or kind == lnkkind:
            if fname in dmap:
                if matchalways or matchfn(fname):
                    results[fname] = entry
            elif (matchalways or matchfn(fname)) and not ignore(fname):
                results[fname] = entry
        elif fname in dmap and (matchalways or matchfn(fname)):
            results[fname] = None

    # step 3: query notable files we don't already know about
    # XXX try not to iterate over the entire dmap
    if normalize:
        # any notable files that have changed case will already be handled
        # above, so just check membership in the foldmap
        notefiles = set((normalize(f, True, True) for f in notefiles
                         if normcase(f) not in foldmap))
    visit = set((f for f in notefiles if (f not in results and matchfn(f)
                                          and (f in dmap or not ignore(f)))))

    if nonnormalset is not None and not fresh_instance:
        if matchalways:
            visit.update(f for f in nonnormalset if f not in results)
            visit.update(f for f in copymap if f not in results)
        else:
            visit.update(f for f in nonnormalset
                         if f not in results and matchfn(f))
            visit.update(f for f in copymap
                         if f not in results and matchfn(f))
    else:
        if matchalways:
            visit.update(f for f, st in dmap.iteritems()
                         if (f not in results and
                             (st[2] < 0 or st[0] != 'n' or fresh_instance)))
            visit.update(f for f in copymap if f not in results)
        else:
            visit.update(f for f, st in dmap.iteritems()
                         if (f not in results and
                             (st[2] < 0 or st[0] != 'n' or fresh_instance)
                             and matchfn(f)))
            visit.update(f for f in copymap
                         if f not in results and matchfn(f))

    audit = pathutil.pathauditor(self._root).check
    auditpass = [f for f in visit if audit(f)]
    auditpass.sort()
    auditfail = visit.difference(auditpass)
    for f in auditfail:
        results[f] = None

    nf = iter(auditpass).next
    for st in util.statfiles([join(f) for f in auditpass]):
        f = nf()
        if st or f in dmap:
            results[f] = st

    for s in subrepos:
        del results[s]
    del results['.hg']
    return results

def overridestatus(
        orig, self, node1='.', node2=None, match=None, ignored=False,
        clean=False, unknown=False, listsubrepos=False):
    listignored = ignored
    listclean = clean
    listunknown = unknown

    def _cmpsets(l1, l2):
        try:
            if 'FSMONITOR_LOG_FILE' in os.environ:
                fn = os.environ['FSMONITOR_LOG_FILE']
                f = open(fn, 'wb')
            else:
                fn = 'fsmonitorfail.log'
                f = self.opener(fn, 'wb')
        except (IOError, OSError):
            self.ui.warn(_('warning: unable to write to %s\n') % fn)
            return

        try:
            for i, (s1, s2) in enumerate(zip(l1, l2)):
                if set(s1) != set(s2):
                    f.write('sets at position %d are unequal\n' % i)
                    f.write('watchman returned: %s\n' % s1)
                    f.write('stat returned: %s\n' % s2)
        finally:
            f.close()

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

    # Maybe we can use this opportunity to update Watchman's state.
    # Mercurial uses workingcommitctx and/or memctx to represent the part of
    # the workingctx that is to be committed. So don't update the state in
    # that case.
    # HG_PENDING is set in the environment when the dirstate is being updated
    # in the middle of a transaction; we must not update our state in that
    # case, or we risk forgetting about changes in the working copy.
    updatestate = (parentworking and match.always() and
                   not isinstance(ctx2, (context.workingcommitctx,
                                         context.memctx)) and
                   'HG_PENDING' not in os.environ)

    try:
        if self._fsmonitorstate.walk_on_invalidate:
            # Use a short timeout to query the current clock.  If that
            # takes too long then we assume that the service will be slow
            # to answer our query.
            # walk_on_invalidate indicates that we prefer to walk the
            # tree ourselves because we can ignore portions that Watchman
            # cannot and we tend to be faster in the warmer buffer cache
            # cases.
            self._watchmanclient.settimeout(0.1)
        else:
            # Give Watchman more time to potentially complete its walk
            # and return the initial clock.  In this mode we assume that
            # the filesystem will be slower than parsing a potentially
            # very large Watchman result set.
            self._watchmanclient.settimeout(
                self._fsmonitorstate.timeout + 0.1)
        startclock = self._watchmanclient.getcurrentclock()
    except Exception as ex:
        self._watchmanclient.clearconnection()
        _handleunavailable(self.ui, self._fsmonitorstate, ex)
        # boo, Watchman failed. bail
        return orig(node1, node2, match, listignored, listclean,
                    listunknown, listsubrepos)

    if updatestate:
        # We need info about unknown files. This may make things slower the
        # first time, but whatever.
        stateunknown = True
    else:
        stateunknown = listunknown

    r = orig(node1, node2, match, listignored, listclean, stateunknown,
             listsubrepos)
    modified, added, removed, deleted, unknown, ignored, clean = r

    if updatestate:
        notefiles = modified + added + removed + deleted + unknown
        self._fsmonitorstate.set(
            self._fsmonitorstate.getlastclock() or startclock,
            _hashignore(self.dirstate._ignore),
            notefiles)

    if not listunknown:
        unknown = []

    # don't do paranoid checks if we're not going to query Watchman anyway
    full = listclean or match.traversedir is not None
    if self._fsmonitorstate.mode == 'paranoid' and not full:
        # run status again and fall back to the old walk this time
        self.dirstate._fsmonitordisable = True

        # shut the UI up
        quiet = self.ui.quiet
        self.ui.quiet = True
        fout, ferr = self.ui.fout, self.ui.ferr
        self.ui.fout = self.ui.ferr = open(os.devnull, 'wb')

        try:
            rv2 = orig(
                node1, node2, match, listignored, listclean, listunknown,
                listsubrepos)
        finally:
            self.dirstate._fsmonitordisable = False
            self.ui.quiet = quiet
            self.ui.fout, self.ui.ferr = fout, ferr

        # clean isn't tested since it's set to True above
        _cmpsets([modified, added, removed, deleted, unknown, ignored, clean],
                 rv2)
        modified, added, removed, deleted, unknown, ignored, clean = rv2

    return scmutil.status(
        modified, added, removed, deleted, unknown, ignored, clean)

def makedirstate(cls):
    class fsmonitordirstate(cls):
        def _fsmonitorinit(self, fsmonitorstate, watchmanclient):
            # _fsmonitordisable is used in paranoid mode
            self._fsmonitordisable = False
            self._fsmonitorstate = fsmonitorstate
            self._watchmanclient = watchmanclient

        def walk(self, *args, **kwargs):
            orig = super(fsmonitordirstate, self).walk
            if self._fsmonitordisable:
                return orig(*args, **kwargs)
            return overridewalk(orig, self, *args, **kwargs)

        def rebuild(self, *args, **kwargs):
            self._fsmonitorstate.invalidate()
            return super(fsmonitordirstate, self).rebuild(*args, **kwargs)

        def invalidate(self, *args, **kwargs):
            self._fsmonitorstate.invalidate()
            return super(fsmonitordirstate, self).invalidate(*args, **kwargs)

    return fsmonitordirstate

def wrapdirstate(orig, self):
    ds = orig(self)
    # only override the dirstate when Watchman is available for the repo
    if util.safehasattr(self, '_fsmonitorstate'):
        ds.__class__ = makedirstate(ds.__class__)
        ds._fsmonitorinit(self._fsmonitorstate, self._watchmanclient)
    return ds

def extsetup(ui):
    wrapfilecache(localrepo.localrepository, 'dirstate', wrapdirstate)
    if sys.platform == 'darwin':
        # An assist for avoiding the dangling-symlink fsevents bug
        extensions.wrapfunction(os, 'symlink', wrapsymlink)

    extensions.wrapfunction(merge, 'update', wrapupdate)

def wrapsymlink(orig, source, link_name):
    ''' if we create a dangling symlink, also touch the parent dir
    to encourage fsevents notifications to work more correctly '''
    try:
        return orig(source, link_name)
    finally:
        try:
            os.utime(os.path.dirname(link_name), None)
        except OSError:
            pass

class state_update(object):
    ''' This context mananger is responsible for dispatching the state-enter
        and state-leave signals to the watchman service '''

    def __init__(self, repo, node, distance, partial):
        self.repo = repo
        self.node = node
        self.distance = distance
        self.partial = partial

    def __enter__(self):
        self._state('state-enter')
        return self

    def __exit__(self, type_, value, tb):
        status = 'ok' if type_ is None else 'failed'
        self._state('state-leave', status=status)

    def _state(self, cmd, status='ok'):
        if not util.safehasattr(self.repo, '_watchmanclient'):
            return
        try:
            commithash = self.repo[self.node].hex()
            self.repo._watchmanclient.command(cmd, {
                'name': 'hg.update',
                'metadata': {
                    # the target revision
                    'rev': commithash,
                    # approximate number of commits between current and target
                    'distance': self.distance,
                    # success/failure (only really meaningful for state-leave)
                    'status': status,
                    # whether the working copy parent is changing
                    'partial': self.partial,
            }})
        except Exception as e:
            # Swallow any errors; fire and forget
            self.repo.ui.log(
                'watchman', 'Exception %s while running %s\n', e, cmd)

# Bracket working copy updates with calls to the watchman state-enter
# and state-leave commands.  This allows clients to perform more intelligent
# settling during bulk file change scenarios
# https://facebook.github.io/watchman/docs/cmd/subscribe.html#advanced-settling
def wrapupdate(orig, repo, node, branchmerge, force, ancestor=None,
               mergeancestor=False, labels=None, matcher=None, **kwargs):

    distance = 0
    partial = True
    if matcher is None or matcher.always():
        partial = False
        wc = repo[None]
        parents = wc.parents()
        if len(parents) == 2:
            anc = repo.changelog.ancestor(parents[0].node(), parents[1].node())
            ancrev = repo[anc].rev()
            distance = abs(repo[node].rev() - ancrev)
        elif len(parents) == 1:
            distance = abs(repo[node].rev() - parents[0].rev())

    with state_update(repo, node, distance, partial):
        return orig(
            repo, node, branchmerge, force, ancestor, mergeancestor,
            labels, matcher, *kwargs)

def reposetup(ui, repo):
    # We don't work with largefiles or inotify
    exts = extensions.enabled()
    for ext in _blacklist:
        if ext in exts:
            ui.warn(_('The fsmonitor extension is incompatible with the %s '
                      'extension and has been disabled.\n') % ext)
            return

    if util.safehasattr(repo, 'dirstate'):
        # We don't work with subrepos either. Note that we can get passed in
        # e.g. a statichttprepo, which throws on trying to access the substate.
        # XXX This sucks.
        try:
            # if repo[None].substate can cause a dirstate parse, which is too
            # slow. Instead, look for a file called hgsubstate,
            if repo.wvfs.exists('.hgsubstate') or repo.wvfs.exists('.hgsub'):
                return
        except AttributeError:
            return

        fsmonitorstate = state.state(repo)
        if fsmonitorstate.mode == 'off':
            return

        try:
            client = watchmanclient.client(repo)
        except Exception as ex:
            _handleunavailable(ui, fsmonitorstate, ex)
            return

        repo._fsmonitorstate = fsmonitorstate
        repo._watchmanclient = client

        # at this point since fsmonitorstate wasn't present, repo.dirstate is
        # not a fsmonitordirstate
        repo.dirstate.__class__ = makedirstate(repo.dirstate.__class__)
        # nuke the dirstate so that _fsmonitorinit and subsequent configuration
        # changes take effect on it
        del repo._filecache['dirstate']
        delattr(repo.unfiltered(), 'dirstate')

        class fsmonitorrepo(repo.__class__):
            def status(self, *args, **kwargs):
                orig = super(fsmonitorrepo, self).status
                return overridestatus(orig, self, *args, **kwargs)

        repo.__class__ = fsmonitorrepo

def wrapfilecache(cls, propname, wrapper):
    """Wraps a filecache property. These can't be wrapped using the normal
    wrapfunction. This should eventually go into upstream Mercurial.
    """
    assert callable(wrapper)
    for currcls in cls.__mro__:
        if propname in currcls.__dict__:
            origfn = currcls.__dict__[propname].func
            assert callable(origfn)
            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)
            currcls.__dict__[propname].func = wrap
            break

    if currcls is object:
        raise AttributeError(
            _("type '%s' has no property '%s'") % (cls, propname))
