# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# __init__.py - fsmonitor initialization and overrides

"""faster status operations with the Watchman file monitor (EXPERIMENTAL)

Integrates the file-watching program Watchman with Mercurial to produce faster
status results.

On a particular Linux system, for a real-world repository with over 400,000
files hosted on ext4, vanilla `@prog@ status` takes 1.3 seconds. On the same
system, with fsmonitor it takes about 0.3 seconds.

fsmonitor requires no configuration -- it will tell Watchman about your
repository as necessary. You'll need to install Watchman from
https://facebook.github.io/watchman/ and make sure it is in your PATH.

fsmonitor is incompatible with the largefiles and eol extensions, and
will disable itself if any of those are active.

The following configuration options exist:

::

    [fsmonitor]
    mode = {off, on}

When `mode = off`, fsmonitor will disable itself (similar to not loading the
extension at all). When `mode = on`, fsmonitor will be enabled (the default).

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
    dirstate-nonnormal-file-threshold = 200

Number of nonnormal files to force obtaining the wlock to update treestate.
Usually status will skip updating treestate if it cannot obtain the wlock,
in some cases that can cause performance issues. This setting allows
status to wait to obtain the wlock to avoid such issues. (default: 200)

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

::

    [fsmonitor]
    watchman-query-lock = (boolean)

If set to true then take a lock when running watchman queries to avoid
overloading watchman.

::
    [fsmonitor]
    wait-full-crawl = true

If set, wait for watchman to complete a full crawl before performing queries.
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
#   *Workaround*: add nested repo paths to your `.gitignore`.
#
# The issues related to nested repos are probably not fundamental
# ones. Patches to fix them are welcome.

from __future__ import absolute_import

import os

from sapling import error, extensions, filesystem, localrepo, pycompat, registrar, util
from sapling.i18n import _

from ..extlib import watchmanclient


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
configitem("fsmonitor", "mode", default="on")
configitem("fsmonitor", "timeout", default=10)
configitem("fsmonitor", "track-ignore-files", default=False)
configitem("fsmonitor", "dirstate-nonnormal-file-threshold", default=200)
configitem("fsmonitor", "watchman-changed-file-threshold", default=200)
configitem("fsmonitor", "warn-fresh-instance", default=False)
configitem("fsmonitor", "fallback-on-watchman-exception", default=True)
configitem("fsmonitor", "tcp", default=False)
configitem("fsmonitor", "tcp-host", default="::1")
configitem("fsmonitor", "tcp-port", default=12300)
configitem("fsmonitor", "watchman-query-lock", default=False)
configitem("fsmonitor", "wait-full-crawl", default=True)

# This extension is incompatible with the following incompatible extensions
# and will disable itself when encountering one of these:
_incompatible_list = ["largefiles", "eol"]


def _handleunavailable(ui, ex):
    """Exception handler for Watchman interaction exceptions"""
    if isinstance(ex, watchmanclient.Unavailable):
        if ex.warn:
            ui.warn(str(ex) + "\n")


def _finddirs(ui, fs):
    """Query watchman for all directories in the working copy"""
    fs._watchmanclient.settimeout(fs._timeout + 0.1)
    result = fs._watchmanclient.command(
        "query",
        {
            "fields": ["name"],
            "expression": [
                "allof",
                ["type", "d"],
                [
                    "not",
                    [
                        "anyof",
                        ["dirname", ui.identity.dotdir()],
                        ["name", ui.identity.dotdir(), "wholename"],
                    ],
                ],
            ],
            "sync_timeout": int(fs._timeout * 1000),
        },
    )
    return list(filter(lambda x: _isutf8(ui, x), result["files"]))


def _isutf8(ui, name):
    if not util.isvalidutf8(name):
        # We don't support non-utf8 file names, so just ignore it.
        # Passing it along to the rest of Mercurial can cause issues
        # since the Python-to-Rust boundary doesn't support
        # surrogate escaped strings.
        name = pycompat.decodeutf8(pycompat.encodeutf8(name, errors="replace"))
        ui.warn(_("skipping invalid utf-8 filename: '%s'\n") % name)
        return False
    return True


def wrappurge(orig, dirstate, match, findfiles, finddirs, includeignored):
    # If includeignored is set, we always need to do a full rewalk.
    if includeignored:
        return orig(dirstate, match, findfiles, finddirs, includeignored)

    ui = dirstate._ui
    files = []
    dirs = []
    errors = []
    if finddirs:
        try:
            dirs = _finddirs(ui, dirstate._fs)
            wvfs = dirstate._repo.wvfs
            dirs = (
                f
                for f in sorted(dirs, reverse=True)
                if (
                    match(f)
                    and not os.listdir(wvfs.join(f))
                    and not dirstate._dirignore(f)
                )
            )
        except Exception:
            ui.debug("fsmonitor: fallback to core purge, " "query dirs failed")
            dirs = None

    if findfiles or dirs is None:
        files, slowdirs, errors = orig(dirstate, match, findfiles, dirs is None, False)
        if dirs is None:
            dirs = slowdirs

    return files, dirs, errors


def makedirstate(repo, dirstate):
    class fsmonitordirstate(dirstate.__class__):
        def _fsmonitorinit(self, repo):
            self._fs = fsmonitorfilesystem(self._root, self, repo)

    dirstate.__class__ = fsmonitordirstate
    dirstate._fsmonitorinit(repo)


class fsmonitorfilesystem(filesystem.physicalfilesystem):
    def __init__(self, root, dirstate, repo):
        super(fsmonitorfilesystem, self).__init__(root, dirstate)

        self._mode = repo.ui.config("fsmonitor", "mode")
        self._timeout = float(repo.ui.config("fsmonitor", "timeout"))
        self._watchmanclient = watchmanclient.getclientforrepo(repo)


def wrapdirstate(orig, self):
    ds = orig(self)
    # only override the dirstate when Watchman is available for the repo
    if hasattr(self, "_fsmonitorok"):
        makedirstate(self, ds)
    return ds


def extsetup(ui):
    extensions.wrapfilecache(localrepo.localrepository, "dirstate", wrapdirstate)
    if pycompat.isdarwin:
        # An assist for avoiding the dangling-symlink fsevents bug
        extensions.wrapfunction(os, "symlink", wrapsymlink)

    extensions.wrapfunction(filesystem, "findthingstopurge", wrappurge)


def wrapsymlink(orig, source, link_name):
    """if we create a dangling symlink, also touch the parent dir
    to encourage fsevents notifications to work more correctly"""
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
    for ext in _incompatible_list:
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
    if repo.ui.config("fsmonitor", "mode") == "off":
        return

    try:
        watchmanclient.createclientforrepo(repo)
    except Exception as ex:
        _handleunavailable(ui, ex)
        return

    repo._fsmonitorok = True

    dirstate, cached = localrepo.isfilecached(repo, "dirstate")
    if cached:
        # at this point since fsmonitorstate wasn't present,
        # repo.dirstate is not a fsmonitordirstate
        makedirstate(repo, dirstate)


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
        - At t3: Run '@prog@ debugrefreshwatchmanclock' before doing anything else
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
