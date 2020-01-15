# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# __init__.py - fsmonitor initialization and overrides

"""faster status operations with the Watchman file monitor (EXPERIMENTAL)

Integrates the file-watching program Watchman with Mercurial to produce faster
status results.

On a particular Linux system, for a real-world repository with over 400,000
files hosted on ext4, vanilla `hg status` takes 1.3 seconds. On the same
system, with fsmonitor it takes about 0.3 seconds.

fsmonitor requires no configuration -- it will tell Watchman about your
repository as necessary. You'll need to install Watchman from
https://facebook.github.io/watchman/ and make sure it is in your PATH.

fsmonitor is incompatible with the largefiles and eol extensions, and
will disable itself if any of those are active.

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
to return results. Defaults to 10.0.

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

::

    [fsmonitor]
    warn_when_unused = (boolean)

Whether to print a warning during certain operations when fsmonitor would be
beneficial to performance but isn't enabled.

::

    [fsmonitor]
    warn_update_file_count = (integer)

If ``warn_when_unused`` is set and fsmonitor isn't enabled, a warning will
be printed during working directory updates if this many files will be
created.

::

    [fsmonitor]
    sockpath = (string)

Posix only: path of unix domain socket to communicate with watchman
The path can contain %i that have to be replaced with user's unix username

::

    [fsmonitor]
    detectrace = (boolean)

If ``detectrace`` is set to True, fsmonitor will spend extra effort detecting
if there are file writes happening during a ``status`` call, and raises an
exception if it finds anything. (default: false)

::

    [fsmonitor]
    track-ignore-files = (boolean)

If set to True, fsmonitor will track ignored files in treestate. This behaves
more correctly if files get unignored, or added to the sparse profile, at the
cost of slowing down status command. Turning it off would make things faster,
at the cast of removing files from ignore patterns (or adding files to sparse
profiles) won't be detected automatically. (default: True)

::

    [fsmonitor]
    watchman-changed-file-threshold = 200

Number of possibly changed files returned by watchman to force a write to
treestate. Set this to a small value updates treestate more frequently,
leading to better performance, at the cost of disk usage. Set this to a large
value would update treestate less frequently, with the downside that
performance might regress in some cases. (default: 200)

::

    [fsmonitor]
    warn-fresh-instance = false

If set to true, warn about fresh instance cases that might slow down
operations.

::

    [fsmonitor]
    fallback-on-watchman-exception = (boolean)

If set to true then it will fallback on the vanilla algorithms for detecting
the state of the working copy. Note that no fallback results in transforming
failures from watchman (or timeouts) in hard failures for the current
operation. (default = true)
"""

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
#   enabled: largefiles, inotify, eol.
# * fsmonitor will produce incorrect results if nested repos exist.
#   *Workaround*: add nested repo paths to your `.hgignore`.
#
# The issues related to nested repos are probably not fundamental
# ones. Patches to fix them are welcome.

from __future__ import absolute_import

import codecs
import hashlib
import os
import stat
import sys
import weakref

from edenscm.mercurial import (
    blackbox,
    context,
    dirstate as dirstatemod,
    encoding,
    error,
    extensions,
    filesystem,
    localrepo,
    match as matchmod,
    pathutil,
    progress,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from ..extlib import pywatchman, watchmanclient
from . import state


# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("fsmonitor", "blacklistusers", default=list)
configitem("fsmonitor", "detectrace", default=False)
configitem("fsmonitor", "mode", default="on")
configitem("fsmonitor", "timeout", default=10)
configitem("fsmonitor", "track-ignore-files", default=True)
configitem("fsmonitor", "walk_on_invalidate", default=False)
configitem("fsmonitor", "watchman-changed-file-threshold", default=200)
configitem("fsmonitor", "warn-fresh-instance", default=False)
configitem("fsmonitor", "fallback-on-watchman-exception", default=True)

# This extension is incompatible with the following blacklisted extensions
# and will disable itself when encountering one of these:
_blacklist = ["largefiles", "eol"]


def _handleunavailable(ui, state, ex):
    """Exception handler for Watchman interaction exceptions"""
    if isinstance(ex, watchmanclient.Unavailable):
        if ex.warn:
            ui.warn(str(ex) + "\n")
        if ex.invalidate:
            state.invalidate(reason="exception")


def _hashignore(ignore):
    """Calculate hash for ignore patterns and filenames

    If this information changes between Mercurial invocations, we can't
    rely on Watchman information anymore and have to re-scan the working
    copy.

    """
    sha1 = hashlib.sha1()
    sha1.update(repr(ignore))
    return sha1.hexdigest()


_watchmanencoding = pywatchman.encoding.get_local_encoding()
_fsencoding = sys.getfilesystemencoding() or sys.getdefaultencoding()
_fixencoding = codecs.lookup(_watchmanencoding) != codecs.lookup(_fsencoding)


def _watchmantofsencoding(path):
    """Fix path to match watchman and local filesystem encoding

    watchman's paths encoding can differ from filesystem encoding. For example,
    on Windows, it's always utf-8.
    """
    try:
        decoded = path.decode(_watchmanencoding)
    except UnicodeDecodeError as e:
        raise error.Abort(str(e), hint="watchman encoding error")

    try:
        encoded = decoded.encode(_fsencoding, "strict")
    except UnicodeEncodeError as e:
        raise error.Abort(str(e))

    return encoded


def _finddirs(fs):
    """Query watchman for all directories in the working copy"""
    state = fs._fsmonitorstate
    fs._watchmanclient.settimeout(state.timeout + 0.1)
    result = fs._watchmanclient.command(
        "query",
        {
            "fields": ["name"],
            "expression": [
                "allof",
                ["type", "d"],
                ["not", ["anyof", ["dirname", ".hg"], ["name", ".hg", "wholename"]]],
            ],
            "sync_timeout": int(state.timeout * 1000),
            "empty_on_fresh_instance": state.walk_on_invalidate,
        },
    )
    return result["files"]


def wrappurge(orig, dirstate, match, findfiles, finddirs, includeignored):
    # If includeignored is set, we always need to do a full rewalk.
    if includeignored:
        return orig(dirstate, match, findfiles, finddirs, includeignored)

    ui = dirstate._ui
    files = []
    dirs = []
    usefastdirs = True
    if finddirs:
        try:
            fastdirs = _finddirs(dirstate._fs)
        except Exception:
            ui.debug("fsmonitor: fallback to core purge, " "query dirs failed")
            usefastdirs = False

    if findfiles or not usefastdirs:
        files, dirs = orig(
            dirstate, match, findfiles, finddirs and not usefastdirs, False
        )

    if finddirs and usefastdirs:
        wvfs = dirstate._repo.wvfs
        dirs = (
            f
            for f in sorted(fastdirs, reverse=True)
            if (
                match(f) and not os.listdir(wvfs.join(f)) and not dirstate._dirignore(f)
            )
        )

    return files, dirs


def _watchmanpid(clock):
    """Get watchman pid from a watchman clock value.

    For example, 'c:1567536997:497:1:21' has pid '497'.
    Return None if `clock` is malformed.
    """
    try:
        pid = int(clock.split(":")[2])
        if pid == 0:
            return None
        else:
            return pid
    except (AttributeError, IndexError):
        return None


def _walk(self, match, event):
    """Replacement for filesystem._walk, hooking into Watchman.

    Whenever listignored is False and the Watchman client is available, use
    Watchman combined with saved state to possibly return only a subset of
    files."""

    state = self._fsmonitorstate
    clock, ignorehash, notefiles = state.get()
    if not clock:
        if state.walk_on_invalidate:
            raise fsmonitorfallback("no clock")
        # Initial NULL clock value, see
        # https://facebook.github.io/watchman/docs/clockspec.html
        clock = "c:0:0"
        notefiles = []

    ignore = self.dirstate._ignore

    # experimental config: experimental.fsmonitor.skipignore
    if not self._ui.configbool("experimental", "fsmonitor.skipignore"):
        if ignorehash and _hashignore(ignore) != ignorehash and clock != "c:0:0":
            # ignore list changed -- can't rely on Watchman state any more
            if state.walk_on_invalidate:
                raise fsmonitorfallback("ignore rules changed")
            notefiles = []
            clock = "c:0:0"

    matchfn = match.matchfn
    matchalways = match.always()
    dmap = self.dirstate._map
    if util.safehasattr(dmap, "_map"):
        # for better performance, directly access the inner dirstate map if the
        # standard dirstate implementation is in use.
        dmap = dmap._map
    if "treestate" in self._repo.requirements:
        # treestate has a fast path to filter out ignored directories.
        ignorevisitdir = self.dirstate._ignore.visitdir

        def dirfilter(path):
            result = ignorevisitdir(path.rstrip("/"))
            return result == "all"

        nonnormalset = self.dirstate._map.nonnormalsetfiltered(dirfilter)
    else:
        nonnormalset = self.dirstate._map.nonnormalset

    event["old_clock"] = clock
    event["old_files"] = blackbox.shortlist(nonnormalset)

    copymap = self.dirstate._map.copymap
    getkind = stat.S_IFMT
    dirkind = stat.S_IFDIR
    regkind = stat.S_IFREG
    lnkkind = stat.S_IFLNK
    join = self.dirstate._join
    normcase = util.normcase
    fresh_instance = False

    exact = False
    if match.isexact():  # match.exact
        exact = True

    if not exact and self.dirstate._checkcase:
        # note that even though we could receive directory entries, we're only
        # interested in checking if a file with the same name exists. So only
        # normalize files if possible.
        normalize = self.dirstate._normalizefile
    else:
        normalize = None

    # step 2: query Watchman
    try:
        # Use the user-configured timeout for the query.
        # Add a little slack over the top of the user query to allow for
        # overheads while transferring the data
        self._watchmanclient.settimeout(state.timeout + 0.1)
        result = self._watchmanclient.command(
            "query",
            {
                "fields": ["mode", "mtime", "size", "exists", "name"],
                "since": clock,
                "expression": [
                    "not",
                    ["anyof", ["dirname", ".hg"], ["name", ".hg", "wholename"]],
                ],
                "sync_timeout": int(state.timeout * 1000),
                "empty_on_fresh_instance": state.walk_on_invalidate,
            },
        )
    except Exception as ex:
        event["is_error"] = True
        _handleunavailable(self._ui, state, ex)
        self._watchmanclient.clearconnection()
        # XXX: Legacy scuba logging. Remove this once the source of truth
        # is moved to the Rust Event.
        self._ui.log("fsmonitor_status", fsmonitor_status="exception")
        if self._ui.configbool("fsmonitor", "fallback-on-watchman-exception"):
            raise fsmonitorfallback("exception during run")
        else:
            raise ex
    else:
        # We need to propagate the last observed clock up so that we
        # can use it for our next query
        event["new_clock"] = result["clock"]
        event["is_fresh"] = result["is_fresh_instance"]
        state.setlastclock(result["clock"])
        state.setlastisfresh(result["is_fresh_instance"])
        if result["is_fresh_instance"]:
            if not self._ui.plain() and self._ui.configbool(
                "fsmonitor", "warn-fresh-instance"
            ):
                oldpid = _watchmanpid(event["old_clock"])
                newpid = _watchmanpid(event["new_clock"])
                if oldpid is not None and newpid is not None:
                    self._ui.warn(
                        _(
                            "warning: watchman has recently restarted (old pid %s, new pid %s) - operation will be slower than usual\n"
                        )
                        % (oldpid, newpid)
                    )
                elif oldpid is None and newpid is not None:
                    self._ui.warn(
                        _(
                            "warning: watchman has recently started (pid %s) - operation will be slower than usual\n"
                        )
                        % (newpid,)
                    )
                else:
                    self._ui.warn(
                        _(
                            "warning: watchman failed to catch up with file change events and requires a full scan - operation will be slower than usual\n"
                        )
                    )

            if state.walk_on_invalidate:
                state.invalidate(reason="fresh_instance")
                raise fsmonitorfallback("fresh instance")
            fresh_instance = True
            # Ignore any prior noteable files from the state info
            notefiles = []
        else:
            count = len(result["files"])
            state.setwatchmanchangedfilecount(count)
            event["new_files"] = blackbox.shortlist(
                (e["name"] for e in result["files"]), count
            )
        # XXX: Legacy scuba logging. Remove this once the source of truth
        # is moved to the Rust Event.
        if event["is_fresh"]:
            self._ui.log("fsmonitor_status", fsmonitor_status="fresh")
        else:
            self._ui.log("fsmonitor_status", fsmonitor_status="normal")

    results = {}

    # for file paths which require normalization and we encounter a case
    # collision, we store our own foldmap
    if normalize:
        foldmap = dict((normcase(k), k) for k in results)

    switch_slashes = pycompat.ossep == "\\"
    # The order of the results is, strictly speaking, undefined.
    # For case changes on a case insensitive filesystem we may receive
    # two entries, one with exists=True and another with exists=False.
    # The exists=True entries in the same response should be interpreted
    # as being happens-after the exists=False entries due to the way that
    # Watchman tracks files.  We use this property to reconcile deletes
    # for name case changes.
    ignorelist = []
    ignorelistappend = ignorelist.append
    for entry in result["files"]:
        fname = entry["name"]
        if _fixencoding:
            fname = _watchmantofsencoding(fname)
        if switch_slashes:
            fname = fname.replace("\\", "/")
        if normalize:
            normed = normcase(fname)
            fname = normalize(fname, True, True)
            foldmap[normed] = fname
        fmode = entry["mode"]
        fexists = entry["exists"]
        kind = getkind(fmode)

        if not fexists:
            # if marked as deleted and we don't already have a change
            # record, mark it as deleted.  If we already have an entry
            # for fname then it was either part of walkexplicit or was
            # an earlier result that was a case change
            if (
                fname not in results
                and fname in dmap
                and (matchalways or matchfn(fname))
            ):
                results[fname] = None
        elif kind == dirkind:
            if fname in dmap and (matchalways or matchfn(fname)):
                results[fname] = None
        elif kind == regkind or kind == lnkkind:
            if fname in dmap:
                if matchalways or matchfn(fname):
                    results[fname] = entry
            else:
                ignored = ignore(fname)
                if ignored:
                    ignorelistappend(fname)
                if (matchalways or matchfn(fname)) and not ignored:
                    results[fname] = entry
        elif fname in dmap and (matchalways or matchfn(fname)):
            results[fname] = None
        elif fname in match.files():
            match.bad(fname, filesystem.badtype(kind))

    # step 3: query notable files we don't already know about
    # XXX try not to iterate over the entire dmap
    if normalize:
        # any notable files that have changed case will already be handled
        # above, so just check membership in the foldmap
        notefiles = set(
            (normalize(f, True, True) for f in notefiles if normcase(f) not in foldmap)
        )
    visit = set(
        (
            f
            for f in notefiles
            if (f not in results and matchfn(f) and (f in dmap or not ignore(f)))
        )
    )

    if not fresh_instance:
        if matchalways:
            visit.update(f for f in nonnormalset if f not in results)
            visit.update(f for f in copymap if f not in results)
        else:
            visit.update(f for f in nonnormalset if f not in results and matchfn(f))
            visit.update(f for f in copymap if f not in results and matchfn(f))
    else:
        if matchalways:
            visit.update(f for f in dmap if f not in results)
            visit.update(f for f in copymap if f not in results)
        else:
            visit.update(f for f in dmap if f not in results and matchfn(f))
            visit.update(f for f in copymap if f not in results and matchfn(f))

    # audit returns False for paths with one of its parent directories being a
    # symlink.
    audit = pathutil.pathauditor(self.dirstate._root, cached=True).check
    auditpass = [f for f in visit if audit(f)]
    auditpass.sort()
    auditfail = visit.difference(auditpass)
    droplist = []
    droplistappend = droplist.append
    for f in auditfail:
        # For auditfail paths, they should be treated as not existed in working
        # copy.
        filestate = dmap.get(f, ("?",))[0]
        if filestate in ("?",):
            # do not exist in working parents, remove them from treestate and
            # avoid walking through them.
            droplistappend(f)
            results.pop(f, None)
        else:
            # tracked, mark as deleted
            results[f] = None

    nf = iter(auditpass).next
    for st in util.statfiles([join(f) for f in auditpass]):
        f = nf()
        if (st and not ignore(f)) or f in dmap:
            results[f] = st
        elif not st:
            # '?' (untracked) file was deleted from the filesystem - remove it
            # from treestate.
            #
            # We can only update the dirstate (and treestate) while holding the
            # wlock. That happens inside poststatus.__call__ -> state.set. So
            # buffer what files to "drop" so state.set can clean them up.
            entry = dmap.get(f, None)
            if entry and entry[0] == "?":
                droplistappend(f)
    # The droplist and ignorelist need to match setlastclock()
    state.setdroplist(droplist)
    state.setignorelist(ignorelist)

    results.pop(".hg", None)
    return results.iteritems()


def overridestatus(
    orig,
    self,
    node1=".",
    node2=None,
    match=None,
    ignored=False,
    clean=False,
    unknown=False,
):
    listignored = ignored
    listclean = clean
    listunknown = unknown

    def _cmpsets(l1, l2):
        try:
            if "FSMONITOR_LOG_FILE" in encoding.environ:
                fn = encoding.environ["FSMONITOR_LOG_FILE"]
                f = open(fn, "wb")
            else:
                fn = "fsmonitorfail.log"
                f = self.opener(fn, "wb")
        except (IOError, OSError):
            self.ui.warn(_("warning: unable to write to %s\n") % fn)
            return

        try:
            for i, (s1, s2) in enumerate(zip(l1, l2)):
                if set(s1) != set(s2):
                    f.write("sets at position %d are unequal\n" % i)
                    f.write("watchman returned: %s\n" % s1)
                    f.write("stat returned: %s\n" % s2)
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
    parentworking = working and ctx1 == self["."]
    match = match or matchmod.always(self.root, self.getcwd())

    # Maybe we can use this opportunity to update Watchman's state.
    # Mercurial uses workingcommitctx and/or memctx to represent the part of
    # the workingctx that is to be committed. So don't update the state in
    # that case.
    # HG_PENDING is set in the environment when the dirstate is being updated
    # in the middle of a transaction; we must not update our state in that
    # case, or we risk forgetting about changes in the working copy.
    updatestate = (
        parentworking
        and match.always()
        and not isinstance(
            ctx2, (context.workingcommitctx, context.overlayworkingctx, context.memctx)
        )
        and "HG_PENDING" not in encoding.environ
    )

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
            self._watchmanclient.settimeout(self._fsmonitorstate.timeout + 0.1)
    except Exception as ex:
        self._watchmanclient.clearconnection()
        _handleunavailable(self.ui, self._fsmonitorstate, ex)
        # boo, Watchman failed.
        if self.ui.configbool("fsmonitor", "fallback-on-watchman-exception"):
            return orig(node1, node2, match, listignored, listclean, listunknown)
        else:
            raise ex

    if updatestate:
        # We need info about unknown files. This may make things slower the
        # first time, but whatever.
        stateunknown = True
    else:
        stateunknown = listunknown

    if updatestate:
        if "treestate" in self.requirements:
            # No need to invalidate fsmonitor state.
            # state.set needs to run before dirstate write, since it changes
            # dirstate (treestate).
            self.addpostdsstatus(poststatustreestate, afterdirstatewrite=False)

    r = orig(node1, node2, match, listignored, listclean, stateunknown)
    modified, added, removed, deleted, unknown, ignored, clean = r

    if not listunknown:
        unknown = []

    # don't do paranoid checks if we're not going to query Watchman anyway
    full = listclean or match.traversedir is not None
    if self._fsmonitorstate.mode == "paranoid" and not full:
        # run status again and fall back to the old walk this time
        self.dirstate._fsmonitordisable = True

        # shut the UI up
        quiet = self.ui.quiet
        self.ui.quiet = True
        fout, ferr = self.ui.fout, self.ui.ferr
        self.ui.fout = self.ui.ferr = open(os.devnull, "wb")

        try:
            rv2 = orig(node1, node2, match, listignored, listclean, listunknown)
        finally:
            self.dirstate._fsmonitordisable = False
            self.ui.quiet = quiet
            self.ui.fout, self.ui.ferr = fout, ferr

        # clean isn't tested since it's set to True above
        _cmpsets([modified, added, removed, deleted, unknown, ignored, clean], rv2)
        modified, added, removed, deleted, unknown, ignored, clean = rv2

    return scmutil.status(modified, added, removed, deleted, unknown, ignored, clean)


def poststatustreestate(wctx, status):
    repo = wctx._repo
    dirstate = repo.dirstate
    oldtrackignored = (dirstate.getmeta("track-ignored") or "1") == "1"
    newtrackignored = repo.ui.configbool("fsmonitor", "track-ignore-files")

    if oldtrackignored != newtrackignored:
        if newtrackignored:
            # Add ignored files to treestate
            ignored = wctx.status(listignored=True).ignored
            repo.ui.debug("start tracking %d ignored files\n" % len(ignored))
            for path in ignored:
                dirstate.needcheck(path)
        else:
            # Remove ignored files from treestate
            ignore = dirstate._ignore
            # pyre-fixme[21]: Could not find `bindings`.
            from bindings import treestate

            repo.ui.debug("stop tracking ignored files\n")
            for path in dirstate._map._tree.walk(
                treestate.NEED_CHECK,
                treestate.EXIST_P1 | treestate.EXIST_P2 | treestate.EXIST_NEXT,
            ):
                if ignore(path):
                    dirstate.delete(path)
        dirstate.setmeta("track-ignored", str(int(newtrackignored)))


def makedirstate(repo, dirstate):
    class fsmonitordirstate(dirstate.__class__):
        def _fsmonitorinit(self, repo):
            self._fs = fsmonitorfilesystem(self._root, self, repo)

        def rebuild(self, *args, **kwargs):
            if not kwargs.get("exact"):
                self._fs._fsmonitorstate.invalidate()
            return super(fsmonitordirstate, self).rebuild(*args, **kwargs)

        def invalidate(self, *args, **kwargs):
            self._fs._fsmonitorstate.invalidate()
            return super(fsmonitordirstate, self).invalidate(*args, **kwargs)

    dirstate.__class__ = fsmonitordirstate
    dirstate._fsmonitorinit(repo)


class fsmonitorfallback(Exception):
    pass


class fsmonitorfilesystem(filesystem.physicalfilesystem):
    def __init__(self, root, dirstate, repo):
        super(fsmonitorfilesystem, self).__init__(root, dirstate)

        # _fsmonitordisable is used in paranoid mode
        self._fsmonitordisable = False
        self._fsmonitorstate = repo._fsmonitorstate
        self._watchmanclient = watchmanclient.getclientforrepo(repo)
        self._repo = weakref.proxy(repo)
        self._ui = repo.ui

    def pendingchanges(self, match=None, listignored=False):
        def bail(reason):
            self._ui.debug("fsmonitor: fallback to core status, %s\n" % reason)
            return super(fsmonitorfilesystem, self).pendingchanges(
                match, listignored=listignored
            )

        if self._fsmonitordisable:
            return bail("fsmonitor disabled")
        if listignored:
            return bail("listing ignored files")
        if not self._watchmanclient.available():
            return bail("client unavailable")

        with progress.spinner(self._ui, "scanning working copy"):
            try:
                # Ideally we'd return the result incrementally, but we need to
                # be able to fall back if watchman fails. So let's consume the
                # whole pendingchanges list upfront.
                return list(self._fspendingchanges(match))
            except fsmonitorfallback as ex:
                return bail(str(ex))

    def _fspendingchanges(self, match=None):
        if match is None:
            match = util.always

        startclock = self._watchmanclient.getcurrentclock()

        self.dirstate._map.preload()
        lookups = []
        results = []
        for fn, st in self._fsmonitorwalk(match):
            changed = self._ischanged(fn, st, lookups)
            if changed:
                results.append(changed[0])
                yield changed

        for changed in self._processlookups(lookups):
            results.append(changed[0])
            yield changed

        oldid = self.dirstate.identity()
        self._fspostpendingfixup(oldid, results, startclock, match)

    def _fspostpendingfixup(self, oldid, changed, startclock, match):
        """update dirstate for files that are actually clean"""
        try:
            repo = self.dirstate._repo

            istreestate = "treestate" in self.dirstate._repo.requirements

            # Only update fsmonitor state if the results aren't filtered
            isfullstatus = not match or match.always()

            # Updating the dirstate is optional so we don't wait on the
            # lock.
            # wlock can invalidate the dirstate, so cache normal _after_
            # taking the lock. This is a bit weird because we're inside the
            # dirstate that is no longer valid.

            # If watchman reports fresh instance, still take the lock,
            # since not updating watchman state leads to very painful
            # performance.
            freshinstance = False
            try:
                freshinstance = self._fs._fsmonitorstate._lastisfresh
            except Exception:
                pass
            if freshinstance:
                repo.ui.debug(
                    "poststatusfixup decides to wait for wlock since watchman reported fresh instance\n"
                )
            with repo.wlock(freshinstance):
                # The dirstate may have been reloaded after the wlock
                # was taken, so load it again.
                newdirstate = repo.dirstate
                if newdirstate.identity() == oldid:
                    # Invalidate fsmonitor.state if dirstate changes. This avoids the
                    # following issue:
                    # 1. pid 11 writes dirstate
                    # 2. pid 22 reads dirstate and inconsistent fsmonitor.state
                    # 3. pid 22 calculates a wrong state
                    # 4. pid 11 writes fsmonitor.state
                    # Because before 1,
                    # 0. pid 11 invalidates fsmonitor.state
                    # will happen.
                    #
                    # To avoid race conditions when reading without a lock, do things
                    # in this order:
                    # 1. Invalidate fsmonitor state
                    # 2. Write dirstate
                    # 3. Write fsmonitor state
                    if isfullstatus:
                        if not istreestate:
                            # Treestate is always in sync and doesn't need this
                            # valdiation.
                            self._fsmonitorstate.invalidate(reason="dirstate_change")
                        else:
                            # Treestate records the fsmonitor state inside the
                            # dirstate, so we need to write it before we call
                            # newdirstate.write()
                            self._updatefsmonitorstate(changed, startclock)

                    self._marklookupsclean()

                    # write changes out explicitly, because nesting
                    # wlock at runtime may prevent 'wlock.release()'
                    # after this block from doing so for subsequent
                    # changing files
                    #
                    # This is a no-op if dirstate is not dirty.
                    tr = repo.currenttransaction()
                    newdirstate.write(tr)

                    # In non-treestate mode write the fsmonitorstate after the
                    # dirstate to avoid the race condition mentioned in the
                    # comment above. In treestate mode this race condition
                    # doesn't exist, and the state is written earlier, so we can
                    # skip it here.
                    if isfullstatus and not istreestate:
                        self._updatefsmonitorstate(changed, startclock)

                    self._newid = newdirstate.identity()
                else:
                    # in this case, writing changes out breaks
                    # consistency, because .hg/dirstate was
                    # already changed simultaneously after last
                    # caching (see also issue5584 for detail)
                    repo.ui.debug("skip marking lookups clean: identity mismatch\n")
        except error.LockError:
            if freshinstance:
                repo.ui.write_err(
                    _(
                        "warning: failed to update watchman state because wlock cannot be obtained\n"
                    )
                )
                repo.ui.write_err(dirstatemod.slowstatuswarning)

    def _updatefsmonitorstate(self, changed, startclock):
        istreestate = "treestate" in self.dirstate._repo.requirements

        clock = self._fsmonitorstate.getlastclock()
        hashignore = None
        notefiles = changed

        if not istreestate:
            if not clock:
                clock = startclock
            hashignore = _hashignore(self.dirstate._ignore)

        # For treestate, the clock and the file state are always consistent - they
        # should not affect "status" correctness, even if they are not the latest
        # state. Changing the clock to None would make the next "status" command
        # slower. Therefore avoid doing that.
        if clock is not None or not istreestate:
            self._fsmonitorstate.set(clock, hashignore, notefiles)

    @util.timefunction("fsmonitorwalk", 0, "_ui")
    def _fsmonitorwalk(self, match):
        fsmonitorevent = {}
        try:
            return _walk(self, match, fsmonitorevent)
        finally:
            try:
                blackbox.log({"fsmonitor": fsmonitorevent})
            except UnicodeDecodeError:
                # test-adding-invalid-utf8.t hits this path
                pass


def wrapdirstate(orig, self):
    ds = orig(self)
    # only override the dirstate when Watchman is available for the repo
    if util.safehasattr(self, "_fsmonitorstate"):
        makedirstate(self, ds)
    return ds


def _racedetect(orig, self, other, s, match, listignored, listclean, listunknown):
    repo = self._repo
    detectrace = repo.ui.configbool("fsmonitor", "detectrace") or util.parsebool(
        encoding.environ.get("HGDETECTRACE", "")
    )
    if detectrace and util.safehasattr(repo.dirstate._fs, "_watchmanclient"):
        state = repo.dirstate._fs._fsmonitorstate
        try:
            startclock = repo.dirstate._fs._watchmanclient.command(
                "clock", {"sync_timeout": int(state.timeout * 1000)}
            )["clock"]
        except Exception as ex:
            repo.ui.warn(_("cannot detect status race: %s\n") % ex)
            detectrace = False
    result = orig(self, other, s, match, listignored, listclean, listunknown)
    if detectrace and util.safehasattr(repo.dirstate._fs, "_fsmonitorstate"):
        raceresult = repo._watchmanclient.command(
            "query",
            {
                "fields": ["name"],
                "since": startclock,
                "expression": [
                    "allof",
                    ["type", "f"],
                    ["not", ["anyof", ["dirname", ".hg"]]],
                ],
                "sync_timeout": int(state.timeout * 1000),
                "empty_on_fresh_instance": True,
            },
        )
        ignore = repo.dirstate._ignore
        racenames = [
            name
            for name in raceresult["files"]
            # hg-checklink*, hg-checkexec* are ignored.
            # Ignored files are allowed unless listignored is set.
            if not name.startswith("hg-check") and (listignored or not ignore(name))
        ]
        if racenames:
            msg = _(
                "[race-detector] files changed when scanning changes in working copy:\n%s"
            ) % "".join("  %s\n" % name for name in sorted(racenames))
            raise error.WorkingCopyRaced(
                msg,
                hint=_(
                    "this is an error because HGDETECTRACE or fsmonitor.detectrace is set to true"
                ),
            )
    return result


def extsetup(ui):
    extensions.wrapfilecache(localrepo.localrepository, "dirstate", wrapdirstate)
    if pycompat.isdarwin:
        # An assist for avoiding the dangling-symlink fsevents bug
        extensions.wrapfunction(os, "symlink", wrapsymlink)

    extensions.wrapfunction(filesystem, "findthingstopurge", wrappurge)
    extensions.wrapfunction(context.workingctx, "_buildstatus", _racedetect)


def wrapsymlink(orig, source, link_name):
    """ if we create a dangling symlink, also touch the parent dir
    to encourage fsevents notifications to work more correctly """
    try:
        return orig(source, link_name)
    finally:
        try:
            os.utime(os.path.dirname(link_name), None)
        except OSError:
            pass


def reposetup(ui, repo):
    # We don't work with largefiles or inotify
    exts = extensions.enabled()
    for ext in _blacklist:
        if ext in exts:
            ui.warn(
                _(
                    "The fsmonitor extension is incompatible with the %s "
                    "extension and has been disabled.\n"
                )
                % ext
            )
            return

    # We only work with local repositories
    if not repo.local():
        return

    # For Eden-backed repositories the eden extension already handles optimizing
    # dirstate operations.  Let the eden extension manage the dirstate in this case.
    if "eden" in repo.requirements:
        return

    # Check if fsmonitor is explicitly disabled for this repository
    fsmonitorstate = state.state(repo)
    if fsmonitorstate.mode == "off":
        return

    try:
        watchmanclient.createclientforrepo(repo)
    except Exception as ex:
        _handleunavailable(ui, fsmonitorstate, ex)
        return

    repo._fsmonitorstate = fsmonitorstate

    dirstate, cached = localrepo.isfilecached(repo, "dirstate")
    if cached:
        # at this point since fsmonitorstate wasn't present,
        # repo.dirstate is not a fsmonitordirstate
        makedirstate(repo, dirstate)

    class fsmonitorrepo(repo.__class__):
        def status(self, *args, **kwargs):
            orig = super(fsmonitorrepo, self).status
            return overridestatus(orig, self, *args, **kwargs)

    repo.__class__ = fsmonitorrepo


@command("debugrefreshwatchmanclock")
def debugrefreshwatchmanclock(ui, repo):
    """refresh watchman clock, assume no changes since the last watchman clock

    This is useful when used together with filesystem snapshots. Typically
    right after restoring a snapshot of a clean working copy, in the following
    pattern::

        - At t0 time: path/repo1's watchman clock is updated to c1
        - At t1: Snapshot path/repo1 with watchman clock = c1
        - At t2: Restore the snapshot to path/repo2
          - Since c1 is no longer a valid watchman clock in repo2, watchman
            would do a re-crawl for correctness.
        - At t3: Run 'hg debugrefreshwatchmanclock' before doing anything else
          in repo2, to update the watchman clock to a valid value (c2).
          - Correctness: changes between c1 (t0) and c2 (t3) are missed.
            - Application can make sure there are no changes by using the
              snapshot feature carefully. For example, make sure the working
              copy is clean before snapshotting, and run
              'debugrefreshwatchmanclock' right after restoring the snapshot.
          - Since c2 is valid in repo2, watchman wouldn't need a re-crawl.

        (t3 > t2 > t1 > t0)
    """

    # Sanity checks
    if not ui.plain():
        raise error.Abort(_("only automation can run this"))

    if "treestate" not in repo.requirements:
        raise error.Abort(_("treestate is required"))

    with repo.wlock(), repo.lock(), repo.transaction("debugrefreshwatchmanclock") as tr:
        # Don't trigger a commitcloud background backup for this.
        repo.ignoreautobackup = True

        # Make sure watchman is watching the repo. This might trigger a
        # filesystem crawl.
        try:
            repo.dirstate._fs._watchmanclient.command("watch")
        except Exception as ex:
            raise error.Abort(_("cannot watch repo: %s") % ex)
        try:
            clock = repo.dirstate._fs._watchmanclient.command(
                "clock", {"sync_timeout": 10 * 1000}
            )["clock"]
        except Exception as ex:
            raise error.Abort(_("cannot get watchman clock: %s") % ex)

        ds = repo.dirstate
        ui.status(_("updating watchman clock from %r to %r\n") % (ds.getclock(), clock))
        ds.setclock(clock)
        ds.write(tr)
