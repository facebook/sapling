# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006, 2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import hashlib
import posixpath
import shutil
import struct

from bindings import (
    checkout as nativecheckout,
    status as nativestatus,
    worker as rustworker,
    workingcopy as rustworkingcopy,
)
from sapling import tracing

from . import (
    copies,
    edenfs,
    error,
    extensions,
    filemerge,
    git,
    i18n,
    json,
    match as matchmod,
    mutation,
    perftrace,
    progress,
    pycompat,
    scmutil,
    util,
    worker,
)
from .i18n import _
from .node import addednodeid, bin, hex, nullhex, nullid, wdirhex
from .pycompat import encodeutf8


class mergestate:
    """track 3-way merge state of individual files

    The merge state is stored on disk when needed. See the
    repostate::merge_state module for details on the format.

    Merge driver run states (experimental):
    u: driver-resolved files unmarked -- needs to be run next time we're about
       to resolve or commit
    m: driver-resolved files marked -- only needs to be run before commit
    s: success/skipped -- does not need to be run any more

    Merge record states (stored in self._state, indexed by filename):
    u: unresolved conflict
    r: resolved conflict
    pu: unresolved path conflict (file conflicts with directory)
    pr: resolved path conflict
    d: driver-resolved conflict

    The resolve command transitions between 'u' and 'r' for conflicts and
    'pu' and 'pr' for path conflicts.

    """

    statepath = "merge/state2"

    @staticmethod
    def clean(repo, node=None, other=None, labels=None, ancestors=None):
        """Initialize a brand new merge state, removing any existing state on
        disk."""
        ms = mergestate(repo)
        ms.reset(node=node, other=other, labels=labels, ancestors=ancestors)
        return ms

    @staticmethod
    def read(repo):
        """Initialize the merge state, reading it from disk."""
        ms = mergestate(repo)
        ms._read(repo._rsrepo.workingcopy().mergestate())
        return ms

    def __init__(self, repo):
        """Initialize the merge state.

        Do not use this directly! Instead call read() or clean()."""
        self._repo = repo
        self._dirty = False

    def reset(self, node=None, other=None, labels=None, ancestors=None):
        shutil.rmtree(self._repo.localvfs.join("merge"), True)

        self._read(rustworkingcopy.mergestate(node, other, labels))

        if ancestors:
            self._ancestors = ancestors

    def _read(self, rust_ms):
        """Analyse each record content to restore a serialized state from disk

        This function process "record" entry produced by the de-serialization
        of on disk file.
        """
        self._rust_ms = rust_ms

        if md := rust_ms.mergedriver():
            self._readmergedriver = md[0]
            self._mdstate = md[1]
        else:
            self._readmergedriver = None
            self._mdstate = "s"

        self._results = {}
        self._dirty = False

        # Note: _ancestors isn't written into the state file since the current
        # state file predates it.
        #
        # It's only needed during `applyupdates` in the initial call to merge,
        # so it's set transiently there. It isn't read during `hg resolve`.
        self._ancestors = None
        for var in ("localctx", "otherctx", "ancestorctxs"):
            if var in vars(self):
                delattr(self, var)

    @util.propertycache
    def mergedriver(self):
        # protect against the following:
        # - A configures a malicious merge driver in their hgrc, then
        #   pauses the merge
        # - A edits their hgrc to remove references to the merge driver
        # - A gives a copy of their entire repo, including .hg, to B
        # - B inspects .hgrc and finds it to be clean
        # - B then continues the merge and the malicious merge driver
        #  gets invoked
        configmergedriver = self._repo.ui.config("experimental", "mergedriver")
        if (
            self._readmergedriver is not None
            and self._readmergedriver != configmergedriver
        ):
            raise error.ConfigError(
                _("merge driver changed since merge started")
                + "\n"
                + _("revert merge driver change or abort merge")
            )

        return configmergedriver

    @util.propertycache
    def localctx(self):
        if self._local is None:
            msg = "localctx accessed but self._local isn't set"
            raise error.ProgrammingError(msg)
        return self._repo[self._local]

    @util.propertycache
    def otherctx(self):
        if self._other is None:
            msg = "otherctx accessed but self._other isn't set"
            raise error.ProgrammingError(msg)
        return self._repo[self._other]

    @util.propertycache
    def ancestorctxs(self):
        if self._ancestors is None:
            raise error.ProgrammingError(
                "ancestorctxs accessed but " "self._ancestors aren't set"
            )
        return [self._repo[node] for node in self._ancestors]

    @util.propertycache
    def _local(self):
        return self._rust_ms.local()

    @util.propertycache
    def _other(self):
        return self._rust_ms.other()

    @util.propertycache
    def _labels(self):
        # Maintain historical behavior of no labels being `None`, not `[]`.
        return self._rust_ms.labels() or None

    def active(self):
        """Whether mergestate is active.

        Returns True if there appears to be mergestate. This is a rough proxy
        for "is a merge in progress."
        """
        # Check local variables before looking at filesystem for performance
        # reasons.
        return (
            bool(self._local)
            or not self._rust_ms.isempty()
            or self._repo.localvfs.exists(self.statepath)
        )

    def commit(self):
        """Write current state on disk (if necessary)"""
        if self._dirty:
            if md := self.mergedriver:
                self._rust_ms.setmergedriver((md, self._mdstate))
            else:
                self._rust_ms.setmergedriver(None)

            self._repo._rsrepo.workingcopy().writemergestate(self._rust_ms)

    def add(self, fcl, fco, fca, fd):
        """add a new (potentially?) conflicting file the merge state
        fcl: file context for local,
        fco: file context for remote,
        fca: file context for ancestors,
        fd:  file path of the resulting merge.

        note: also write the local version to the `.hg/merge` directory.
        """
        if fcl.isabsent():
            hash = nullhex
        else:
            hash = hex(hashlib.sha1(encodeutf8(fcl.path())).digest())
            self._repo.localvfs.write("merge/" + hash, fcl.data())
        self._rust_ms.insert(
            fd,
            [
                "u",
                hash,
                fcl.path(),
                fca.path(),
                hex(fca.filenode()),
                fco.path(),
                hex(fco.filenode()),
                fcl.flags(),
            ],
        )
        self._rust_ms.setextra(fd, "ancestorlinknode", hex(fca.node()))
        self._dirty = True

    def addpath(self, path, frename, forigin):
        """add a new conflicting path to the merge state
        path:    the path that conflicts
        frename: the filename the conflicting file was renamed to
        forigin: origin of the file ('l' or 'r' for local/remote)
        """
        self._rust_ms.insert(path, ["pu", frename, forigin])
        self._dirty = True

    def __contains__(self, dfile):
        return self._rust_ms.contains(dfile)

    def __getitem__(self, dfile):
        return self._rust_ms.get(dfile)[0]

    def __iter__(self):
        return iter(sorted(self._rust_ms.files()))

    def files(self):
        return self._rust_ms.files()

    def mark(self, dfile, state):
        self._rust_ms.setstate(dfile, state)
        self._dirty = True

    def mdstate(self):
        return self._mdstate

    def unresolved(self):
        """Obtain the paths of unresolved files."""
        return self._rust_ms.files(("u", "pu"))

    def driverresolved(self):
        """Obtain the paths of driver-resolved files."""
        return self._rust_ms.files(("d",))

    def extras(self, filename):
        return self._rust_ms.extras(filename)

    def _resolve(self, preresolve, dfile, wctx):
        """rerun merge process for file path `dfile`"""
        if self[dfile] in "rd":
            return True, 0
        stateentry = self._rust_ms.get(dfile)
        _state, hexdnode, lfile, afile, hexanode, ofile, hexonode, flags = stateentry
        dnode = bin(hexdnode)
        onode = bin(hexonode)
        anode = bin(hexanode)
        octx = self._repo[self._other]
        extras = self.extras(dfile)
        anccommitnode = extras.get("ancestorlinknode")
        if anccommitnode:
            actx = self._repo[anccommitnode]
        else:
            actx = None

        self._repo.ui.log(
            "merge_resolve", "resolving %s, preresolve = %s", dfile, preresolve
        )
        fcd = self._filectxorabsent(dnode, wctx, dfile)
        fco = self._filectxorabsent(onode, octx, ofile)
        # TODO: move this to filectxorabsent
        fca = self._repo.filectx(afile, fileid=anode, changeid=actx)
        # "premerge" x flags
        flo = fco.flags()
        fla = fca.flags()
        if "x" in flags + flo + fla and "l" not in flags + flo + fla:
            if fca.node() == nullid and flags != flo:
                if preresolve:
                    self._repo.ui.warn(
                        _(
                            "warning: cannot merge flags for %s "
                            "without common ancestor - keeping local flags\n"
                        )
                        % afile
                    )
            elif flags == fla:
                flags = flo
        if preresolve:
            # restore local
            if dnode != nullid:
                f = self._repo.localvfs("merge/" + hexdnode)
                wctx[dfile].write(f.read(), flags)
                f.close()
            else:
                wctx[dfile].remove(ignoremissing=True)
            while True:
                try:
                    complete, r, deleted = filemerge.premerge(
                        self._repo,
                        wctx,
                        self._local,
                        lfile,
                        fcd,
                        fco,
                        fca,
                        labels=self._labels,
                    )
                    break
                except error.RetryFileMerge as ex:
                    fcd = ex.fcd
        else:
            while True:
                try:
                    complete, r, deleted = filemerge.filemerge(
                        self._repo,
                        wctx,
                        self._local,
                        lfile,
                        fcd,
                        fco,
                        fca,
                        labels=self._labels,
                    )
                    break
                except error.RetryFileMerge as ex:
                    fcd = ex.fcd
        if r is None:
            # no real conflict
            self._rust_ms.remove(dfile)
            self._dirty = True
        elif not r:
            self.mark(dfile, "r")

        if complete:
            action = None
            if deleted:
                if fcd.isabsent():
                    # dc: local picked. Need to drop if present, which may
                    # happen on re-resolves.
                    action = "f"
                else:
                    # cd: remote picked (or otherwise deleted)
                    action = "r"
            else:
                if fcd.isabsent():  # dc: remote picked
                    action = "g"
                elif fco.isabsent():  # cd: local picked
                    if dfile in self.localctx:
                        action = "am"
                    else:
                        action = "a"
                # else: regular merges (no action necessary)
            self._results[dfile] = r, action

        return complete, r

    def _filectxorabsent(self, node, ctx, f):
        assert len(node) == len(nullid)
        if node == nullid:
            return filemerge.absentfilectx(ctx, f)
        else:
            return ctx[f]

    def preresolve(self, dfile, wctx):
        """run premerge process for dfile

        Returns whether the merge is complete, and the exit code."""
        return self._resolve(True, dfile, wctx)

    def resolve(self, dfile, wctx):
        """run merge process (assuming premerge was run) for dfile

        Returns the exit code of the merge."""
        return self._resolve(False, dfile, wctx)[1]

    def counts(self):
        """return counts for updated, merged and removed files in this
        session"""
        updated, merged, removed = 0, 0, 0
        for r, action in pycompat.itervalues(self._results):
            if r is None:
                updated += 1
            elif r == 0:
                if action == "r":
                    removed += 1
                else:
                    merged += 1
        return updated, merged, removed

    def unresolvedcount(self):
        """get unresolved count for this merge (persistent)"""
        return len(list(self.unresolved()))

    def actions(self):
        """return lists of actions to perform on the dirstate"""
        actions = {"r": [], "f": [], "a": [], "am": [], "g": []}
        for f, (r, action) in pycompat.iteritems(self._results):
            if action is not None:
                actions[action].append((f, None, "merge result"))
        return actions

    def recordactions(self):
        """record remove/add/get actions in the dirstate"""
        branchmerge = self._repo.dirstate.p2() != nullid
        recordupdates(self._repo, self.actions(), branchmerge)

    def queueremove(self, f):
        """queues a file to be removed from the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, "r"

    def queueadd(self, f):
        """queues a file to be added to the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, "a"

    def queueget(self, f):
        """queues a file to be marked modified in the dirstate

        Meant for use by custom merge drivers."""
        self._results[f] = 0, "g"


def _getcheckunknownconfig(repo, section, name):
    config = repo.ui.config(section, name)
    valid = ["abort", "ignore", "warn"]
    if config not in valid:
        validstr = ", ".join(["'" + v + "'" for v in valid])
        raise error.ConfigError(
            _("%s.%s not valid " "('%s' is none of %s)")
            % (section, name, config, validstr)
        )
    return config


def _checkunknownfile(repo, wctx, mctx, f, f2=None):
    if wctx.isinmemory():
        # Nothing to do in IMM because nothing in the "working copy" can be an
        # unknown file.
        #
        # Note that we should bail out here, not in ``_checkunknownfiles()``,
        # because that function does other useful work.
        return False

    if f2 is None:
        f2 = f
    mfctx = mctx[f2]
    return (
        repo.wvfs.audit.check(f)
        and repo.wvfs.isfileorlink(f)
        and repo.dirstate.normalize(f) not in repo.dirstate
        and mfctx.filelog().cmp(mfctx.filenode(), wctx[f].data())
    )


class _unknowndirschecker:
    """
    Look for any unknown files or directories that may have a path conflict
    with a file.  If any path prefix of the file exists as a file or link,
    then it conflicts.  If the file itself is a directory that contains any
    file that is not tracked, then it conflicts.

    Returns the shortest path at which a conflict occurs, or None if there is
    no conflict.
    """

    def __init__(self):
        # A set of paths known to be good.  This prevents repeated checking of
        # dirs.  It will be updated with any new dirs that are checked and found
        # to be safe.
        self._unknowndircache = set()

        # A set of paths that are known to be absent.  This prevents repeated
        # checking of subdirectories that are known not to exist. It will be
        # updated with any new dirs that are checked and found to be absent.
        self._missingdircache = set()

    def __call__(self, repo, wctx, f):
        if wctx.isinmemory():
            # Nothing to do in IMM for the same reason as ``_checkunknownfile``.
            return False

        # Check for path prefixes that exist as unknown files.
        for p in reversed(list(util.finddirs(f))):
            if p in self._missingdircache:
                return
            if p in self._unknowndircache:
                continue
            if repo.wvfs.audit.check(p):
                if (
                    repo.wvfs.isfileorlink(p)
                    and repo.dirstate.normalize(p) not in repo.dirstate
                ):
                    return p
                if not repo.wvfs.lexists(p):
                    self._missingdircache.add(p)
                    return
                self._unknowndircache.add(p)

        # Check if the file conflicts with a directory containing unknown files.
        if repo.wvfs.audit.check(f) and repo.wvfs.isdir(f):
            # Does the directory contain any files that are not in the dirstate?
            for p, dirs, files in repo.wvfs.walk(f):
                for fn in files:
                    relf = repo.dirstate.normalize(posixpath.join(p, fn))
                    if relf not in repo.dirstate:
                        return f
        return None


@perftrace.tracefunc("Check Unknown Files")
def _checkunknownfiles(repo, wctx, mctx, force, actions):
    """
    Considers any actions that care about the presence of conflicting unknown
    files. For some actions, the result is to abort; for others, it is to
    choose a different action.
    """
    fileconflicts = set()
    pathconflicts = set()
    warnconflicts = set()
    abortconflicts = set()
    unknownconfig = _getcheckunknownconfig(repo, "merge", "checkunknown")
    ignoredconfig = _getcheckunknownconfig(repo, "merge", "checkignored")
    pathconfig = repo.ui.configbool("experimental", "merge.checkpathconflicts")

    progiter = lambda itr: progress.each(repo.ui, itr, "check untracked")

    if not force:

        def collectconflicts(conflicts, config):
            if config == "abort":
                abortconflicts.update(conflicts)
            elif config == "warn":
                warnconflicts.update(conflicts)

        checkunknowndirs = _unknowndirschecker()
        count = 0
        for f, (m, args, msg) in progiter(pycompat.iteritems(actions)):
            if m in ("c", "dc"):
                count += 1
                if _checkunknownfile(repo, wctx, mctx, f):
                    fileconflicts.add(f)
                elif pathconfig and f not in wctx:
                    path = checkunknowndirs(repo, wctx, f)
                    if path is not None:
                        pathconflicts.add(path)
            elif m == "dg":
                count += 1
                if _checkunknownfile(repo, wctx, mctx, f, args[0]):
                    fileconflicts.add(f)

        allconflicts = fileconflicts | pathconflicts
        ignoredconflicts = set([c for c in allconflicts if repo.dirstate._ignore(c)])
        unknownconflicts = allconflicts - ignoredconflicts
        collectconflicts(ignoredconflicts, ignoredconfig)
        collectconflicts(unknownconflicts, unknownconfig)
    else:
        for f, (m, args, msg) in progiter(pycompat.iteritems(actions)):
            if m == "cm":
                fl2, anc = args
                different = _checkunknownfile(repo, wctx, mctx, f)
                if repo.dirstate._ignore(f):
                    config = ignoredconfig
                else:
                    config = unknownconfig

                # The behavior when force is True is described by this table:
                #  config  different    |    action    backup
                #    *         n        |      get        n
                #    *         y        |     merge       -
                #   abort      y        |     merge       -   (1)
                #   warn       y        |  warn + get     y
                #  ignore      y        |      get        y
                #
                # (1) this is probably the wrong behavior here -- we should
                #     probably abort, but some actions like rebases currently
                #     don't like an abort happening in the middle of
                #     merge.update/goto.
                if not different:
                    actions[f] = ("g", (fl2, False), "remote created")
                elif config == "abort":
                    actions[f] = (
                        "m",
                        (f, f, None, False, anc),
                        "remote differs from untracked local",
                    )
                else:
                    if config == "warn":
                        warnconflicts.add(f)
                    actions[f] = ("g", (fl2, True), "remote created")

    for f in sorted(abortconflicts):
        warn = repo.ui.warn
        if f in pathconflicts:
            if repo.wvfs.isfileorlink(f):
                warn(_("%s: untracked file conflicts with directory\n") % f)
            else:
                warn(_("%s: untracked directory conflicts with file\n") % f)
        else:
            warn(_("%s: untracked file differs\n") % f)
    if abortconflicts:
        raise error.Abort(
            _(
                "untracked files in working directory "
                "differ from files in requested revision"
            )
        )

    for f in sorted(warnconflicts):
        if repo.wvfs.isfileorlink(f):
            repo.ui.warn(_("%s: replacing untracked file\n") % f)
        else:
            repo.ui.warn(_("%s: replacing untracked files in directory\n") % f)

    for f, (m, args, msg) in pycompat.iteritems(actions):
        if m == "c":
            backup = (
                f in fileconflicts
                or f in pathconflicts
                or any(p in pathconflicts for p in util.finddirs(f))
            )
            (flags,) = args
            actions[f] = ("g", (flags, backup), msg)


def _forgetremoved(wctx, mctx, branchmerge):
    """
    Forget removed files

    If we're jumping between revisions (as opposed to merging), and if
    neither the working directory nor the target rev has the file,
    then we need to remove it from the dirstate, to prevent the
    dirstate from listing the file when it is no longer in the
    manifest.

    If we're merging, and the other revision has removed a file
    that is not present in the working directory, we need to mark it
    as removed.
    """

    actions = {}
    m = "f"
    if branchmerge:
        m = "r"
    for f in wctx.deleted():
        if f not in mctx:
            actions[f] = m, None, "forget deleted"

    if not branchmerge:
        for f in wctx.removed():
            if f not in mctx:
                actions[f] = "f", None, "forget removed"

    return actions


def driverpreprocess(repo, ms, wctx, labels=None):
    """run the preprocess step of the merge driver, if any

    This is currently not implemented -- it's an extension point."""
    return True


def driverconclude(repo, ms, wctx, labels=None):
    """run the conclude step of the merge driver, if any

    This is currently not implemented -- it's an extension point."""
    return True


def _filesindirs(repo, manifest, dirs):
    """
    Generator that yields pairs of all the files in the manifest that are found
    inside the directories listed in dirs, and which directory they are found
    in.
    """
    for dir in dirs:
        dirmatch = matchmod.match(repo.root, "", include=[dir + "/**"])
        for f in manifest.matches(dirmatch):
            yield f, dir


def checkpathconflicts(repo, wctx, mctx, actions):
    """
    Check if any actions introduce path conflicts in the repository, updating
    actions to record or handle the path conflict accordingly.
    """
    mf = wctx.manifest()

    # The set of local files that conflict with a remote directory.
    localconflicts = set()

    # The set of directories that conflict with a remote file, and so may cause
    # conflicts if they still contain any files after the merge.
    remoteconflicts = set()

    # The set of directories that appear as both a file and a directory in the
    # remote manifest.  These indicate an invalid remote manifest, which
    # can't be updated to cleanly.
    invalidconflicts = set()

    # The set of directories that contain files that are being created.
    createdfiledirs = set()

    # The set of files deleted by all the actions.
    deletedfiles = set()

    for f, (m, args, msg) in actions.items():
        if m in ("c", "dc", "m", "cm"):
            # This action may create a new local file.
            createdfiledirs.update(util.finddirs(f))
            if mf.hasdir(f):
                # The file aliases a local directory.  This might be ok if all
                # the files in the local directory are being deleted.  This
                # will be checked once we know what all the deleted files are.
                remoteconflicts.add(f)
        # Track the names of all deleted files.
        if m == "r":
            deletedfiles.add(f)
        if m == "m":
            f1, f2, fa, move, anc = args
            if move:
                deletedfiles.add(f1)
        if m == "dm":
            f2, flags = args
            deletedfiles.add(f2)

    # Check all directories that contain created files for path conflicts.
    for p in createdfiledirs:
        if p in mf:
            if p in mctx:
                # A file is in a directory which aliases both a local
                # and a remote file.  This is an internal inconsistency
                # within the remote manifest.
                invalidconflicts.add(p)
            else:
                # A file is in a directory which aliases a local file.
                # We will need to rename the local file.
                localconflicts.add(p)
        if p in actions and actions[p][0] in ("c", "dc", "m", "cm"):
            # The file is in a directory which aliases a remote file.
            # This is an internal inconsistency within the remote
            # manifest.
            invalidconflicts.add(p)

    # Rename all local conflicting files that have not been deleted.
    for p in localconflicts:
        if p not in deletedfiles:
            ctxname = str(wctx).rstrip("+")
            pnew = util.safename(p, ctxname, wctx, set(actions.keys()))
            actions[pnew] = ("pr", (p,), "local path conflict")
            actions[p] = ("p", (pnew, "l"), "path conflict")

    if remoteconflicts:
        # Check if all files in the conflicting directories have been removed.
        ctxname = str(mctx).rstrip("+")
        for f, p in _filesindirs(repo, mf, remoteconflicts):
            if f not in deletedfiles:
                m, args, msg = actions[p]
                pnew = util.safename(p, ctxname, wctx, set(actions.keys()))
                if m in ("dc", "m"):
                    # Action was merge, just update target.
                    actions[pnew] = (m, args, msg)
                else:
                    # Action was create, change to renamed get action.
                    fl = args[0]
                    actions[pnew] = ("dg", (p, fl), "remote path conflict")
                actions[p] = ("p", (pnew, "r"), "path conflict")
                remoteconflicts.remove(p)
                break

    if invalidconflicts:
        for p in invalidconflicts:
            repo.ui.warn(_("%s: is both a file and a directory\n") % p)
        raise error.Abort(_("destination manifest contains path conflicts"))


def manifestmerge(
    repo,
    wctx,
    p2,
    pa,
    branchmerge,
    force,
    acceptremote,
    followcopies,
    forcefulldiff=False,
):
    """
    Merge wctx and p2 with ancestor pa and generate merge action list

    branchmerge and force are as passed in to update
    acceptremote = accept the incoming changes without prompting
    """
    copy, movewithdir, diverge, renamedelete, dirmove = {}, {}, {}, {}, {}

    # manifests fetched in order are going to be faster, so prime the caches
    [x.manifest() for x in sorted(wctx.parents() + [p2, pa], key=scmutil.intrev)]

    if followcopies:
        ret = copies.mergecopies(repo, wctx, p2, pa)
        copy, movewithdir, diverge, renamedelete, dirmove = ret

    boolbm = pycompat.bytestr(bool(branchmerge))
    boolf = pycompat.bytestr(bool(force))
    shouldsparsematch = hasattr(repo, "sparsematch") and (
        "eden" not in repo.requirements or "edensparse" in repo.requirements
    )
    sparsematch = getattr(repo, "sparsematch", None) if shouldsparsematch else None
    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(" branchmerge: %s, force: %s\n" % (boolbm, boolf))
    repo.ui.debug(" ancestor: %s, local: %s, remote: %s\n" % (pa, wctx, p2))

    m1, m2, ma = wctx.manifest(), p2.manifest(), pa.manifest()
    copied = set(copy.values())
    copied.update(movewithdir.values())

    matcher = None

    # Don't use m2-vs-ma optimization if:
    # - ma is the same as m1 or m2, which we're just going to diff again later
    # - The caller specifically asks for a full diff, which is useful during bid
    #   merge.
    if pa not in ([wctx, p2] + wctx.parents()) and not forcefulldiff:
        # Identify which files are relevant to the merge, so we can limit the
        # total m1-vs-m2 diff to just those files. This has significant
        # performance benefits in large repositories.
        relevantfiles = set(ma.diff(m2).keys())

        # For copied and moved files, we need to add the source file too.
        for copykey, copyvalue in pycompat.iteritems(copy):
            if copyvalue in relevantfiles:
                relevantfiles.add(copykey)
        for movedirkey in movewithdir:
            relevantfiles.add(movedirkey)
        matcher = scmutil.matchfiles(repo, relevantfiles)

    # For sparse repos, attempt to use the sparsematcher to narrow down
    # calculation.  Consider a typical rebase:
    #
    #     o new master (m1, wctx, rebase destination)
    #     .
    #     . o (m2, rebase source)
    #     |/
    #     o (ma)
    #
    # Split diff(m1, m2) into 2 parts:
    # - diff(m2, ma) is small, and cannot use sparseamtcher for correctness.
    #   (see test-sparse-rebase.t)
    # - diff(m1, ma) is potentially huge, and can use sparsematcher.
    elif sparsematch is not None and not forcefulldiff:
        if branchmerge:
            relevantfiles = set(ma.diff(m2).keys())
            for copykey, copyvalue in pycompat.iteritems(copy):
                if copyvalue in relevantfiles:
                    relevantfiles.add(copykey)
            for movedirkey in movewithdir:
                relevantfiles.add(movedirkey)
            filesmatcher = scmutil.matchfiles(repo, relevantfiles)
        else:
            filesmatcher = None

        revs = {repo.dirstate.p1(), repo.dirstate.p2(), pa.node(), p2.node()}
        revs -= {nullid, None}
        sparsematcher = sparsematch(*list(revs))

        # use sparsematcher to make diff(m1, ma) less expensive.
        if filesmatcher is not None:
            sparsematcher = matchmod.unionmatcher([sparsematcher, filesmatcher])
        matcher = matchmod.intersectmatchers(matcher, sparsematcher)

    with perftrace.trace("Manifest Diff"):
        if hasattr(repo, "resettreefetches"):
            repo.resettreefetches()
        diff = m1.diff(m2, matcher=matcher)
        perftrace.tracevalue("Differences", len(diff))
        if hasattr(repo, "resettreefetches"):
            perftrace.tracevalue("Tree Fetches", repo.resettreefetches())

    if matcher is None:
        matcher = matchmod.always("", "")

    actions = {}
    # (n1, fl1) = "local"
    # (n2, fl2) = "remote"
    for f, ((n1, fl1), (n2, fl2)) in pycompat.iteritems(diff):
        if n1 and n2:  # file exists on both local and remote side
            if f not in ma:
                fa = copy.get(f, None)
                if fa is not None:
                    actions[f] = (
                        "m",
                        (f, f, fa, False, pa.node()),
                        "both renamed from " + fa,
                    )
                else:
                    actions[f] = ("m", (f, f, None, False, pa.node()), "both created")
            else:
                a = ma[f]
                fla = ma.flags(f)
                nol = "l" not in fl1 + fl2 + fla
                if n2 == a and fl2 == fla:
                    actions[f] = ("k", (), "remote unchanged")
                elif n1 == a and fl1 == fla:  # local unchanged - use remote
                    if fl1 == fl2:
                        actions[f] = ("g", (fl2, False), "remote is newer")
                    else:
                        actions[f] = ("rg", (fl2, False), "flag differ")
                elif nol and n2 == a:  # remote only changed 'x'
                    actions[f] = ("e", (fl2,), "update permissions")
                elif nol and n1 == a:  # local only changed 'x'
                    actions[f] = ("g", (fl1, False), "remote is newer")
                else:  # both changed something
                    actions[f] = ("m", (f, f, f, False, pa.node()), "versions differ")
        elif n1:  # file exists only on local side
            if f in copied:
                pass  # we'll deal with it on m2 side
            elif f in movewithdir:  # directory rename, move local
                f2 = movewithdir[f]
                if f2 in m2:
                    actions[f2] = (
                        "m",
                        (f, f2, None, True, pa.node()),
                        "remote directory rename, both created",
                    )
                else:
                    actions[f2] = (
                        "dm",
                        (f, fl1),
                        "remote directory rename - move from " + f,
                    )
            elif f in copy:
                f2 = copy[f]
                if f2 in m2:
                    actions[f] = (
                        "m",
                        (f, f2, f2, False, pa.node()),
                        "local copied/moved from " + f2,
                    )
                else:
                    # copy source doesn't exist - treat this as
                    # a change/delete conflict.
                    actions[f] = (
                        "cd",
                        (f, None, f2, False, pa.node()),
                        "prompt changed/deleted copy source",
                    )
            elif f in ma:  # clean, a different, no remote
                if n1 != ma[f]:
                    if acceptremote:
                        actions[f] = ("r", None, "remote delete")
                    else:
                        actions[f] = (
                            "cd",
                            (f, None, f, False, pa.node()),
                            "prompt changed/deleted",
                        )
                elif n1 == addednodeid:
                    # This extra 'a' is added by working copy manifest to mark
                    # the file as locally added. We should forget it instead of
                    # deleting it.
                    actions[f] = ("f", None, "remote deleted")
                else:
                    actions[f] = ("r", None, "other deleted")
        elif n2:  # file exists only on remote side
            if f in copied:
                pass  # we'll deal with it on m1 side
            elif f in movewithdir:
                f2 = movewithdir[f]
                if f2 in m1:
                    actions[f2] = (
                        "m",
                        (f2, f, None, False, pa.node()),
                        "local directory rename, both created",
                    )
                else:
                    actions[f2] = (
                        "dg",
                        (f, fl2),
                        "local directory rename - get from " + f,
                    )
            elif f in copy:
                f2 = copy[f]
                if f2 in m2:
                    actions[f] = (
                        "m",
                        (f2, f, f2, False, pa.node()),
                        "remote copied from " + f2,
                    )
                else:
                    actions[f] = (
                        "m",
                        (f2, f, f2, True, pa.node()),
                        "remote moved from " + f2,
                    )
            elif f not in ma:
                # local unknown, remote created: the logic is described by the
                # following table:
                #
                # force  branchmerge  different  |  action
                #   n         *           *      |   create
                #   y         n           *      |   create
                #   y         y           n      |   create
                #   y         y           y      |   merge
                #
                # Checking whether the files are different is expensive, so we
                # don't do that when we can avoid it.
                if not force:
                    actions[f] = ("c", (fl2,), "remote created")
                elif not branchmerge:
                    actions[f] = ("c", (fl2,), "remote created")
                else:
                    actions[f] = (
                        "cm",
                        (fl2, pa.node()),
                        "remote created, get or merge",
                    )
            elif n2 != ma[f]:
                df = None
                for d in dirmove:
                    if f.startswith(d):
                        # new file added in a directory that was moved
                        df = dirmove[d] + f[len(d) :]
                        break
                if df is not None and df in m1:
                    actions[df] = (
                        "m",
                        (df, f, f, False, pa.node()),
                        "local directory rename - respect move from " + f,
                    )
                elif acceptremote:
                    actions[f] = ("c", (fl2,), "remote recreating")
                else:
                    actions[f] = (
                        "dc",
                        (None, f, f, False, pa.node()),
                        "prompt deleted/changed",
                    )

    if repo.ui.configbool("experimental", "merge.checkpathconflicts"):
        # If we are merging, look for path conflicts.
        checkpathconflicts(repo, wctx, p2, actions)

    return actions, diverge, renamedelete


def _resolvetrivial(repo, wctx, mctx, ancestor, actions):
    """Resolves false conflicts where the nodeid changed but the content
    remained the same."""

    for f, (m, args, msg) in pycompat.listitems(actions):
        if m == "cd" and f in ancestor and not wctx[f].cmp(ancestor[f]):
            # local did change but ended up with same content
            actions[f] = "r", None, "prompt same"
        elif m == "dc" and f in ancestor and not mctx[f].cmp(ancestor[f]):
            # remote did change but ended up with same content
            del actions[f]  # don't get = keep local deleted


@perftrace.tracefunc("Calculate Updates")
@util.timefunction("calculateupdates", 0, "ui")
def calculateupdates(
    repo,
    wctx,
    mctx,
    ancestors,
    branchmerge,
    force,
    acceptremote,
    followcopies,
):
    """Calculate the actions needed to merge mctx into wctx using ancestors"""

    if len(ancestors) == 1:  # default
        actions, diverge, renamedelete = manifestmerge(
            repo,
            wctx,
            mctx,
            ancestors[0],
            branchmerge,
            force,
            acceptremote,
            followcopies,
        )
        _checkunknownfiles(repo, wctx, mctx, force, actions)

    else:  # only when merge.preferancestor=* - the default
        repo.ui.note(
            _("note: merging %s and %s using bids from ancestors %s\n")
            % (wctx, mctx, _(" and ").join(pycompat.bytestr(anc) for anc in ancestors))
        )

        # Call for bids
        fbids = {}  # mapping filename to bids (action method to list af actions)
        diverge, renamedelete = None, None
        for ancestor in ancestors:
            repo.ui.note(_("\ncalculating bids for ancestor %s\n") % ancestor)
            actions, diverge1, renamedelete1 = manifestmerge(
                repo,
                wctx,
                mctx,
                ancestor,
                branchmerge,
                force,
                acceptremote,
                followcopies,
                forcefulldiff=True,
            )
            _checkunknownfiles(repo, wctx, mctx, force, actions)

            # Track the shortest set of warning on the theory that bid
            # merge will correctly incorporate more information
            if diverge is None or len(diverge1) < len(diverge):
                diverge = diverge1
            if renamedelete is None or len(renamedelete) < len(renamedelete1):
                renamedelete = renamedelete1

            for f, a in sorted(pycompat.iteritems(actions)):
                m, args, msg = a
                repo.ui.debug(" %s: %s -> %s\n" % (f, msg, m))
                if f in fbids:
                    d = fbids[f]
                    if m in d:
                        d[m].append(a)
                    else:
                        d[m] = [a]
                else:
                    fbids[f] = {m: [a]}

        # Pick the best bid for each file
        repo.ui.note(_("\nauction for merging merge bids\n"))
        actions = {}
        dms = []  # filenames that have dm actions
        for f, bids in sorted(fbids.items()):
            # bids is a mapping from action method to list af actions
            # Consensus?
            if len(bids) == 1:  # all bids are the same kind of method
                m, l = list(bids.items())[0]
                if all(a == l[0] for a in l[1:]):  # len(bids) is > 1
                    repo.ui.note(_(" %s: consensus for %s\n") % (f, m))
                    actions[f] = l[0]
                    if m == "dm":
                        dms.append(f)
                    continue
            # If keep is an option, just do it.
            if "k" in bids:
                repo.ui.note(_(" %s: picking 'keep' action\n") % f)
                actions[f] = bids["k"][0]
                continue
            # If there are gets and they all agree [how could they not?], do it.
            if "g" in bids:
                ga0 = bids["g"][0]
                if all(a == ga0 for a in bids["g"][1:]):
                    repo.ui.note(_(" %s: picking 'get' action\n") % f)
                    actions[f] = ga0
                    continue
            # Same for symlink->file change
            if "rg" in bids:
                ga0 = bids["rg"][0]
                if all(a == ga0 for a in bids["rg"][1:]):
                    repo.ui.note(_(" %s: picking 'remove-then-get' action\n") % f)
                    actions[f] = ga0
                    continue
            # TODO: Consider other simple actions such as mode changes
            # Handle inefficient democrazy.
            repo.ui.note(_(" %s: multiple bids for merge action:\n") % f)
            for m, l in sorted(bids.items()):
                for _f, args, msg in l:
                    repo.ui.note("  %s -> %s\n" % (msg, m))
            # Pick random action. TODO: Instead, prompt user when resolving
            m, l = list(bids.items())[0]
            repo.ui.warn(_(" %s: ambiguous merge - picked %s action\n") % (f, m))
            actions[f] = l[0]
            if m == "dm":
                dms.append(f)
            continue
        # Work around 'dm' that can cause multiple actions for the same file
        for f in dms:
            dm, (f0, flags), msg = actions[f]
            assert dm == "dm", dm
            if f0 in actions and actions[f0][0] == "r":
                # We have one bid for removing a file and another for moving it.
                # These two could be merged as first move and then delete ...
                # but instead drop moving and just delete.
                del actions[f]
        repo.ui.note(_("end of auction\n\n"))

    _resolvetrivial(repo, wctx, mctx, ancestors[0], actions)

    if wctx.rev() is None:
        fractions = _forgetremoved(wctx, mctx, branchmerge)
        actions.update(fractions)

    return actions, diverge, renamedelete


def removeone(repo, wctx, f):
    wctx[f].audit()
    try:
        wctx[f].remove(ignoremissing=True)
    except OSError as inst:
        repo.ui.warn(_("update failed to remove %s: %s!\n") % (f, inst.strerror))


def batchremove(repo, wctx, actions):
    """apply removes to the working directory

    yields tuples for progress updates
    """
    verbose = repo.ui.verbose
    cwd = pycompat.getcwdsafe()
    i = 0
    for f, args, msg in actions:
        repo.ui.debug(" %s: %s -> r\n" % (f, msg))
        if verbose:
            repo.ui.note(_("removing %s\n") % f)
        removeone(repo, wctx, f)
        if i == 100:
            yield i, 0, f
            i = 0
        i += 1
    if i > 0:
        yield i, 0, f

    if cwd and not pycompat.getcwdsafe():
        # cwd was removed in the course of removing files; print a helpful
        # warning.
        repo.ui.warn(
            _(
                "current directory was removed\n"
                "(consider changing to repo root: %s)\n"
            )
            % repo.root
        )


def updateone(repo, fctxfunc, wctx, f, flags, backup=False, backgroundclose=False):
    if backup:
        # If a file or directory exists with the same name, back that
        # up.  Otherwise, look to see if there is a file that conflicts
        # with a directory this file is in, and if so, back that up.
        absf = repo.wjoin(f)
        if not repo.wvfs.lexists(f):
            for p in util.finddirs(f):
                if repo.wvfs.isfileorlink(p):
                    absf = repo.wjoin(p)
                    break
        orig = scmutil.origpath(repo.ui, repo, absf)
        if repo.wvfs.lexists(absf):
            util.rename(absf, orig)
    fctx = fctxfunc(f)
    if fctx.flags() == "m" and not wctx.isinmemory():
        # Do not handle submodules for on-disk checkout here.
        # They are handled separately.
        return 0
    wctx[f].clearunknown()
    data = fctx.data()
    wctx[f].write(data, flags, backgroundclose=backgroundclose)

    return len(data)


def batchget(repo, mctx, wctx, actions):
    """apply gets to the working directory

    mctx is the context to get from

    yields tuples for progress updates
    """
    verbose = repo.ui.verbose
    fctx = mctx.filectx
    ui = repo.ui
    i = 0
    size = 0
    with repo.wvfs.backgroundclosing(ui, expectedcount=len(actions)):
        for f, (flags, backup), msg in actions:
            repo.ui.debug(" %s: %s -> g\n" % (f, msg))
            if verbose:
                repo.ui.note(_("getting %s\n") % f)

            size += updateone(repo, fctx, wctx, f, flags, backup, backgroundclose=True)
            if i == 100:
                yield i, size, f
                i = 0
                size = 0
            i += 1
    if i > 0:
        yield i, size, f


@perftrace.tracefunc("Apply Updates")
@util.timefunction("applyupdates", 0, "ui")
def applyupdates(repo, actions, wctx, mctx, overwrite, labels=None, ancestors=None):
    """apply the merge action list to the working directory

    wctx is the working copy context
    mctx is the context to be merged into the working copy

    Return a tuple of counts (updated, merged, removed, unresolved) that
    describes how many files were affected by the update.
    """
    perftrace.tracevalue("Actions", sum(len(v) for k, v in pycompat.iteritems(actions)))

    updated, merged, removed = 0, 0, 0

    ms = mergestate.clean(
        repo,
        node=wctx.p1().node(),
        other=mctx.node(),
        # Ancestor can include the working copy, so we use this helper:
        ancestors=[scmutil.contextnodesupportingwdir(c) for c in ancestors]
        if ancestors
        else None,
        labels=labels,
    )

    moves = []
    for m, l in actions.items():
        l.sort()

    # 'cd' and 'dc' actions are treated like other merge conflicts
    mergeactions = sorted(actions["cd"])
    mergeactions.extend(sorted(actions["dc"]))
    mergeactions.extend(actions["m"])
    for f, args, msg in mergeactions:
        f1, f2, fa, move, anc = args
        if f1 is None:
            fcl = filemerge.absentfilectx(wctx, fa)
        else:
            repo.ui.debug(" preserving %s for resolve of %s\n" % (f1, f))
            fcl = wctx[f1]
        if f2 is None:
            fco = filemerge.absentfilectx(mctx, fa)
        else:
            fco = mctx[f2]
        actx = repo[anc]
        if fa in actx:
            fca = actx[fa]
        else:
            # TODO: move to absentfilectx
            fca = repo.filectx(f1, changeid=nullid, fileid=nullid)
        # Skip submodules for now
        if fcl.flags() == "m" or fco.flags() == "m":
            continue
        ms.add(fcl, fco, fca, f)
        if f1 != f and move:
            moves.append(f1)

    # remove renamed files after safely stored
    for f in moves:
        if wctx[f].lexists():
            repo.ui.debug("removing %s\n" % f)
            wctx[f].audit()
            wctx[f].remove()

    numupdates = sum(len(l) for m, l in actions.items() if m != "k")
    z = 0

    def userustworker():
        return "remotefilelog" in repo.requirements and not wctx.isinmemory()

    rustworkers = userustworker()

    # record path conflicts
    with progress.bar(
        repo.ui, _("updating"), _("files"), numupdates
    ) as prog, repo.ui.timesection("updateworker"):
        for f, args, msg in actions["p"]:
            f1, fo = args
            s = repo.ui.status
            s(
                _(
                    "%s: path conflict - a file or link has the same name as a "
                    "directory\n"
                )
                % f
            )
            if fo == "l":
                s(_("the local file has been renamed to %s\n") % f1)
            else:
                s(_("the remote file has been renamed to %s\n") % f1)
            s(_("resolve manually then use '@prog@ resolve --mark %s'\n") % f)
            ms.addpath(f, f1, fo)
            z += 1
            prog.value = (z, f)

        # Flush any pending data to disk before forking workers, so the workers
        # don't all flush duplicate data.
        repo.commitpending()

        # remove in parallel (must come before resolving path conflicts and
        # getting)
        if rustworkers:
            # Removing lots of files very quickly is known to cause FSEvents to
            # lose events which forces watchman to recrwawl the entire
            # repository. For very large repository, this can take many
            # minutes, slowing down all the other tools that rely on it. Thus
            # add a config that can be tweaked to specifically reduce the
            # amount of concurrency.
            numworkers = repo.ui.configint(
                "experimental", "numworkersremover", worker._numworkers(repo.ui)
            )
            remover = rustworker.removerworker(repo.wvfs.base, numworkers)
            for f, args, msg in actions["r"] + actions["rg"]:
                # The remove method will either return immediately or block if
                # the internal worker queue is full.
                remover.remove(f)
                z += 1
                prog.value = (z, f)
            retry = remover.wait()
            for f in retry:
                repo.ui.debug("retrying %s\n" % f)
                removeone(repo, wctx, f)
        else:
            for i, size, item in batchremove(repo, wctx, actions["r"] + actions["rg"]):
                z += i
                prog.value = (z, item)
        # "rg" actions are counted in updated below
        removed = len(actions["r"])

        # resolve path conflicts (must come before getting)
        for f, args, msg in actions["pr"]:
            repo.ui.debug(" %s: %s -> pr\n" % (f, msg))
            (f0,) = args
            if wctx[f0].lexists():
                repo.ui.note(_("moving %s to %s\n") % (f0, f))
                wctx[f].audit()
                wctx[f].write(wctx.filectx(f0).data(), wctx.filectx(f0).flags())
                wctx[f0].remove()
            z += 1
            prog.value = (z, f)

        # get in parallel
        writesize = 0

        if rustworkers:
            numworkers = repo.ui.configint(
                "experimental", "numworkerswriter", worker._numworkers(repo.ui)
            )

            writer = rustworker.writerworker(
                repo.fileslog.filestore, repo.wvfs.base, numworkers
            )
            fctx = mctx.filectx
            slinkfix = pycompat.iswindows and repo.wvfs._cansymlink
            slinks = []
            for f, (flags, backup), msg in actions["g"] + actions["rg"]:
                if slinkfix and "l" in flags:
                    slinks.append(f)
                fnode = fctx(f).filenode()
                # The write method will either return immediately or block if
                # the internal worker queue is full.
                writer.write(f, fnode, flags)

                z += 1
                prog.value = (z, f)

            writesize, retry = writer.wait()
            for f, flag in retry:
                repo.ui.debug("retrying %s\n" % f)
                writesize += updateone(repo, fctx, wctx, f, flag)
            if slinkfix:
                nativecheckout.fixsymlinks(slinks, repo.wvfs.base)
        else:
            for i, size, item in batchget(
                repo, mctx, wctx, actions["g"] + actions["rg"]
            ):
                z += i
                writesize += size
                prog.value = (z, item)
        updated = len(actions["g"]) + len(actions["rg"])
        perftrace.tracebytes("Disk Writes", writesize)

        # forget (manifest only, just log it) (must come first)
        for f, args, msg in actions["f"]:
            repo.ui.debug(" %s: %s -> f\n" % (f, msg))
            z += 1
            prog.value = (z, f)

        # re-add (manifest only, just log it)
        for f, args, msg in actions["a"]:
            repo.ui.debug(" %s: %s -> a\n" % (f, msg))
            z += 1
            prog.value = (z, f)

        # re-add/mark as modified (manifest only, just log it)
        for f, args, msg in actions["am"]:
            repo.ui.debug(" %s: %s -> am\n" % (f, msg))
            z += 1
            prog.value = (z, f)

        # keep (noop, just log it)
        for f, args, msg in actions["k"]:
            repo.ui.debug(" %s: %s -> k\n" % (f, msg))
            # no progress

        # directory rename, move local
        for f, args, msg in actions["dm"]:
            repo.ui.debug(" %s: %s -> dm\n" % (f, msg))
            z += 1
            prog.value = (z, f)
            f0, flags = args
            repo.ui.note(_("moving %s to %s\n") % (f0, f))
            wctx[f].audit()
            wctx[f].write(wctx.filectx(f0).data(), flags)
            wctx[f0].remove()
            updated += 1

        # local directory rename, get
        for f, args, msg in actions["dg"]:
            repo.ui.debug(" %s: %s -> dg\n" % (f, msg))
            z += 1
            prog.value = (z, f)
            f0, flags = args
            repo.ui.note(_("getting %s to %s\n") % (f0, f))
            wctx[f].write(mctx.filectx(f0).data(), flags)
            updated += 1

        # exec
        for f, args, msg in actions["e"]:
            repo.ui.debug(" %s: %s -> e\n" % (f, msg))
            z += 1
            prog.value = (z, f)
            (flags,) = args
            wctx[f].audit()
            wctx[f].setflags("l" in flags, "x" in flags)
            updated += 1

        perftrace.tracevalue("Deleted Files", removed)
        perftrace.tracevalue("Written Files", updated)

        # the ordering is important here -- ms.mergedriver will raise if the
        # merge driver has changed, and we want to be able to bypass it when
        # overwrite is True
        usemergedriver = not overwrite and mergeactions and ms.mergedriver

        if usemergedriver:
            ms.commit()
            with repo.ui.timesection("mergedriver"):
                # This will return False if the function raises an exception.
                failed = not driverpreprocess(repo, ms, wctx, labels=labels)
            driverresolved = [f for f in ms.driverresolved()]

            repo.ui.log("command_metrics", mergedriver_num_files=len(driverresolved))

            # If preprocess() marked any files as driver-resolved and we're
            # merging in-memory, abort on the assumption that driver scripts
            # require the working directory.
            if driverresolved and wctx.isinmemory():
                errorstr = (
                    "some of your files require mergedriver to run, "
                    "which in-memory merge does not support"
                )
                raise error.InMemoryMergeConflictsError(
                    errorstr,
                    type=error.InMemoryMergeConflictsError.TYPE_MERGEDRIVER,
                    paths=driverresolved,
                )

            # NOTE(phillco): This used to say "the driver might leave some files unresolved",
            # but this actually only handles the case where preprocess() fails. A preprocess()
            # script can also leave files unmarked without failing.
            unresolvedf = set(ms.unresolved())
            if failed:
                # Preprocess failed, so don't proceed in either case.
                if wctx.isinmemory():
                    raise error.InMemoryMergeConflictsError(
                        "preprocess() raised an exception",
                        type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
                        paths=list(unresolvedf),
                    )
                else:
                    # XXX setting unresolved to at least 1 is a hack to make sure we
                    # error out
                    return updated, merged, removed, max(len(unresolvedf), 1)
            newactions = []
            for f, args, msg in mergeactions:
                if f in unresolvedf:
                    newactions.append((f, args, msg))
            mergeactions = newactions

        try:
            # premerge
            tocomplete = []
            completed = []
            for f, args, msg in mergeactions:
                repo.ui.debug(" %s: %s -> m (premerge)\n" % (f, msg))
                z += 1
                prog.value = (z, f)
                wfctx = wctx[f]
                wfctx.audit()
                # Skip submodules for now
                try:
                    if wfctx.flags() == "m":
                        continue
                except error.ManifestLookupError:
                    # Cannot check the flags - ignore.
                    # This code path is hit by test-rebase-inmemory-conflicts.t.
                    pass
                complete, r = ms.preresolve(f, wctx)
                if not complete:
                    numupdates += 1
                    tocomplete.append((f, args, msg))
                else:
                    completed.append(f)

            # merge
            files = []
            for f, args, msg in tocomplete:
                repo.ui.debug(" %s: %s -> m (merge)\n" % (f, msg))
                z += 1
                prog.value = (z, f)
                ms.resolve(f, wctx)
                files.append(f)
            reponame = repo.ui.config("fbscmquery", "reponame")
            command = " ".join(util.shellquote(a) for a in pycompat.sysargv)
            repo.ui.log(
                "manualmergefiles",
                manual_merge_files=",".join(files),
                auto_merge_files=",".join(completed),
                command=command,
                repo=reponame,
            )
            if files:
                repo.ui.log(
                    "merge_conflicts",
                    command=repo.ui.cmdname,
                    full_command=command,
                    dest_hex=_gethex(wctx),
                    src_hex=_gethex(mctx),
                    repo=reponame,
                    manual_merge_files_count=len(files),
                    manual_merge_files=",".join(files),
                )
        finally:
            ms.commit()

        unresolved = ms.unresolvedcount()

        if usemergedriver and not unresolved and ms.mdstate() != "s":
            with repo.ui.timesection("mergedriver"):
                if not driverconclude(repo, ms, wctx, labels=labels):
                    # XXX setting unresolved to at least 1 is a hack to make
                    # sure we error out
                    unresolved = max(unresolved, 1)

            ms.commit()

        msupdated, msmerged, msremoved = ms.counts()
        updated += msupdated
        merged += msmerged
        removed += msremoved

        extraactions = ms.actions()
        if extraactions:
            # A same file might exist both in extraactions["r"] (to remove)
            # list, and actions["g"] (to create) list. Remove them from
            # actions["g"] to avoid conflicts.
            extraremoved = {item[0] for item in extraactions["r"]}
            if extraremoved:
                actions["g"] = [
                    item for item in actions["g"] if item[0] not in extraremoved
                ]

            mfiles = set(a[0] for a in actions["m"])
            for k, acts in pycompat.iteritems(extraactions):
                actions[k].extend(acts)
                # Remove these files from actions['m'] as well. This is
                # important because in recordupdates, files in actions['m'] are
                # processed after files in other actions, and the merge driver
                # might add files to those actions via extraactions above. This
                # can lead to a file being recorded twice, with poor results.
                # This is especially problematic for actions['r'] (currently
                # only possible with the merge driver in the initial merge
                # process; interrupted merges don't go through this flow).
                #
                # The real fix here is to have indexes by both file and action
                # so that when the action for a file is changed it is
                # automatically reflected in the other action lists. But that
                # involves a more complex data structure, so this will do for
                # now.
                #
                # We don't need to do the same operation for 'dc' and 'cd'
                # because those lists aren't consulted again.
                mfiles.difference_update(a[0] for a in acts)

            actions["m"] = [a for a in actions["m"] if a[0] in mfiles]

    return updated, merged, removed, unresolved


def recordupdates(repo, actions, branchmerge):
    "record merge actions to the dirstate"

    total = sum(map(len, actions.values()))

    with progress.bar(repo.ui, _("recording"), _("files"), total) as prog:
        # remove (must come first)
        for f, args, msg in actions.get("r", []):
            if branchmerge:
                repo.dirstate.remove(f)
            else:
                repo.dirstate.delete(f)
            prog.value += 1

        # forget (must come first)
        for f, args, msg in actions.get("f", []):
            repo.dirstate.untrack(f)
            prog.value += 1

        # resolve path conflicts
        copied = repo.dirstate.copies()
        for f, args, msg in actions.get("pr", []):
            (f0,) = args
            origf0 = copied.get(f0, f0)
            repo.dirstate.add(f)
            repo.dirstate.copy(origf0, f)
            if f0 == origf0:
                repo.dirstate.remove(f0)
            else:
                repo.dirstate.delete(f0)
            prog.value += 1

        # re-add
        for f, args, msg in actions.get("a", []):
            repo.dirstate.add(f)
            prog.value += 1

        # re-add/mark as modified
        for f, args, msg in actions.get("am", []):
            if branchmerge:
                repo.dirstate.normallookup(f)
            else:
                repo.dirstate.add(f)
            prog.value += 1

        # exec change
        for f, args, msg in actions.get("e", []):
            repo.dirstate.normallookup(f)
            prog.value += 1

        # keep
        for f, args, msg in actions.get("k", []):
            prog.value += 1

        # get
        for f, args, msg in actions.get("g", []) + actions.get("rg", []):
            if branchmerge:
                repo.dirstate.otherparent(f)
            else:
                repo.dirstate.normal(f)
            prog.value += 1

        # merge
        for f, args, msg in actions.get("m", []):
            f1, f2, fa, move, anc = args
            if branchmerge:
                # We've done a branch merge, mark this file as merged
                # so that we properly record the merger later
                repo.dirstate.merge(f)
                if f1 != f2:  # copy/rename
                    if move:
                        repo.dirstate.remove(f1)
                    if f1 != f:
                        repo.dirstate.copy(f1, f)
                    else:
                        repo.dirstate.copy(f2, f)
            else:
                # We've update-merged a locally modified file, so
                # we set the dirstate to emulate a normal checkout
                # of that file some time in the past. Thus our
                # merge will appear as a normal local file
                # modification.
                if f2 == f:  # file not locally copied/moved
                    repo.dirstate.normallookup(f)
                if move:
                    repo.dirstate.delete(f1)
            prog.value += 1

        # directory rename, move local
        for f, args, msg in actions.get("dm", []):
            f0, flag = args
            if branchmerge:
                repo.dirstate.add(f)
                repo.dirstate.remove(f0)
                repo.dirstate.copy(f0, f)
            else:
                repo.dirstate.normal(f)
                repo.dirstate.delete(f0)
            prog.value += 1

        # directory rename, get
        for f, args, msg in actions.get("dg", []):
            f0, flag = args
            if branchmerge:
                repo.dirstate.add(f)
                repo.dirstate.copy(f0, f)
            else:
                repo.dirstate.normal(f)
            prog.value += 1


def _logupdatedistance(ui, repo, node):
    """Logs the update distance, if configured"""
    # internal config: merge.recordupdatedistance
    if not ui.configbool("merge", "recordupdatedistance", default=True):
        return

    try:
        # The passed in node might actually be a rev, and if it's -1, that
        # doesn't play nicely with revsets later because it resolve to the tip
        # commit.
        node = repo[node].node()
        revdistance = abs(repo["."].rev() - repo[node].rev())
        if revdistance == 0:
            distance = 0
        elif revdistance >= 100000:
            # Calculating real distance is too slow.
            # Use an approximate.
            distance = ((revdistance + 500) / 1000) * 1000
        else:
            distance = len(repo.revs("(%n %% .) + (. %% %n)", node, node))
        repo.ui.log("update_size", update_distance=distance)
    except Exception:
        # error may happen like: RepoLookupError: unknown revision '-1'
        pass


def querywatchmanrecrawls(repo):
    try:
        path = repo.root
        x, x, x, p = util.popen4("watchman debug-status")
        stdout, stderr = p.communicate()
        data = json.loads(stdout)
        for root in data["roots"]:
            if root["path"] == path:
                count = root["recrawl_info"]["count"]
                if root["recrawl_info"]["should-recrawl"] is True:
                    count += 1
                return count
        return 0
    except Exception:
        return 0


def _prefetchlazychildren(repo, node):
    """Prefetch children for ``node`` for lazy changelog.

    This helps making committing on ``node`` offline-friendly.
    """
    # Prefetch lazy node to make offline commit possible.
    if "lazychangelog" in repo.storerequirements:
        # node might be a revision number.
        if not isinstance(node, bytes):
            node = repo[node].node()
        dag = repo.changelog.dag
        if node in dag.mastergroup():
            # See D30004908. Pre-calcualte children(node) so
            # commit on node is more offline friendly.
            try:
                childrennodes = list(dag.children([node]))
            except Exception as e:
                tracing.debug(
                    "cannot resolve children of %s: %r" % (hex(node), e),
                    target="checkout::prefetch",
                )
            else:
                tracing.debug(
                    "children of %s: %s" % (hex(node), [hex(n) for n in childrennodes]),
                    target="checkout::prefetch",
                )
        else:
            tracing.debug(
                "skip prefetch because %s is not in master (lazy) group" % hex(node),
                target="checkout::prefetch",
            )

    else:
        tracing.debug(
            "skip prefetch for non-lazychangelog",
            target="checkout::prefetch",
        )


def goto(
    repo,
    node,
    force=False,
    labels=None,
    updatecheck=None,
):
    _logupdatedistance(repo.ui, repo, node)
    _prefetchlazychildren(repo, node)

    if not force:
        # TODO: remove the default once all callers that pass force=False pass
        # a value for updatecheck. We may want to allow updatecheck='abort' to
        # better suppport some of these callers.
        if updatecheck is None:
            updatecheck = "linear"
        assert updatecheck in ("none", "linear", "noconflict")

    if edenfs.requirement in repo.requirements:
        from . import eden_update

        return eden_update.update(
            repo,
            node,
            force=force,
            labels=labels,
            updatecheck=updatecheck,
        )

    # If we're doing the initial checkout from null, let's use the new fancier
    # nativecheckout, since it has more efficient fetch mechanics.
    # git backend only supports nativecheckout at present.
    isclonecheckout = repo["."].node() == nullid

    if (
        repo.ui.configbool("experimental", "nativecheckout")
        or (repo.ui.configbool("clone", "nativecheckout") and isclonecheckout)
        or git.isgitstore(repo)
    ):
        wc = repo[None]

        if (
            not isclonecheckout
            and (force or updatecheck != "noconflict")
            and (wc.dirty(missing=True) or mergestate.read(repo).active())
        ):
            fallbackcheckout = (
                "Working copy is dirty and --clean specified - not supported yet"
            )
        elif not hasattr(repo.fileslog, "filestore"):
            fallbackcheckout = "Repo does not have remotefilelog"
        else:
            fallbackcheckout = None

        if fallbackcheckout:
            repo.ui.debug("Not using native checkout: %s\n" % fallbackcheckout)
        else:
            # If the user is attempting to checkout for the first time, let's assume
            # they don't have any pending changes and let's do a force checkout.
            # This makes it much faster, by skipping the entire "check for unknown
            # files" and "check for conflicts" code paths, and makes it so they
            # aren't blocked by pending files and have to purge+clone over and over.
            if isclonecheckout:
                force = True

            p1 = wc.parents()[0]
            p2 = repo[node]

            with repo.wlock():
                ret = donativecheckout(
                    repo,
                    p1,
                    p2,
                    force,
                    wc,
                    querywatchmanrecrawls(repo),
                )
                if git.isgitformat(repo):
                    git.submodulecheckout(p2, force=force)
                return ret

    return _update(
        repo,
        node,
        force=force,
        labels=labels,
        updatecheck=updatecheck,
    )


def merge(
    repo,
    node,
    force=False,
    ancestor=None,
    mergeancestor=False,
    labels=None,
    wc=None,
):
    _prefetchlazychildren(repo, node)

    return _update(
        repo,
        node,
        branchmerge=True,
        ancestor=ancestor,
        mergeancestor=mergeancestor,
        force=force,
        labels=labels,
        wc=wc,
    )


@perftrace.tracefunc("Update")
@util.timefunction("mergeupdate", 0, "ui")
def _update(
    repo,
    node,
    branchmerge=False,
    force=False,
    ancestor=None,
    mergeancestor=False,
    labels=None,
    updatecheck=None,
    wc=None,
):
    """
    Perform a merge between the working directory and the given node

    node = the node to update to
    branchmerge = whether to merge between branches
    force = whether to force branch merging or file overwriting
    mergeancestor = whether it is merging with an ancestor. If true,
      we should accept the incoming changes for any prompts that occur.
      If false, merging with an ancestor (fast-forward) is only allowed
      between different named branches. This flag is used by rebase extension
      as a temporary fix and should be avoided in general.
    labels = labels to use for base, local and other

    The table below shows all the behaviors of the update command given the
    -c/--check and -C/--clean or no options, whether the working directory is
    dirty, whether a revision is specified, and the relationship of the parent
    rev to the target rev (linear or not). Match from top first. The -n
    option doesn't exist on the command line, but represents the
    commands.update.check=noconflict option.

    This logic is tested by test-update-branches.t.

    -c  -C  -n  -m  dirty  rev  linear  |  result
     y   y   *   *    *     *     *     |    (1)
     y   *   y   *    *     *     *     |    (1)
     y   *   *   y    *     *     *     |    (1)
     *   y   y   *    *     *     *     |    (1)
     *   y   *   y    *     *     *     |    (1)
     *   *   y   y    *     *     *     |    (1)
     *   *   *   *    *     n     n     |     x
     *   *   *   *    n     *     *     |    ok
     n   n   n   n    y     *     y     |   merge
     n   n   n   n    y     y     n     |    (2)
     n   n   n   y    y     *     *     |   merge
     n   n   y   n    y     *     *     |  merge if no conflict
     n   y   n   n    y     *     *     |  discard
     y   n   n   n    y     *     *     |    (3)

    x = can't happen
    * = don't-care
    1 = incompatible options (checked in commands.py)
    2 = abort: uncommitted changes (commit or goto --clean to discard changes)
    3 = abort: uncommitted changes (checked in commands.py)

    The merge is performed inside ``wc``, a workingctx-like objects. It defaults
    to repo[None] if None is passed.

    Return the same tuple as applyupdates().
    """

    # This function used to find the default destination if node was None, but
    # that's now in destutil.py.
    assert node is not None

    # Positive indication we aren't using eden fastpath for eden integration tests.
    if edenfs.requirement in repo.requirements:
        repo.ui.debug("falling back to non-eden update code path: merge\n")

    with repo.wlock():
        prerecrawls = querywatchmanrecrawls(repo)

        if wc is None:
            wc = repo[None]
        pl = wc.parents()
        p1 = pl[0]
        pas = [None]
        if ancestor is not None:
            pas = [repo[ancestor]]

        overwrite = force and not branchmerge

        p2 = repo[node]

        fp1, fp2, xp1, xp2 = p1.node(), p2.node(), str(p1), str(p2)

        if pas[0] is None:
            if repo.ui.configlist("merge", "preferancestor") == ["*"]:
                cahs = repo.changelog.commonancestorsheads(p1.node(), p2.node())
                pas = [repo[anc] for anc in (sorted(cahs) or [nullid])]
            else:
                pas = [p1.ancestor(p2, warn=branchmerge)]

        ### check phase
        if not overwrite:
            if len(pl) > 1:
                raise error.Abort(_("outstanding uncommitted merge"))
            ms = mergestate.read(repo)
            if list(ms.unresolved()):
                raise error.Abort(_("outstanding merge conflicts"))
        if branchmerge:
            if pas == [p2]:
                raise error.Abort(
                    _("merging with a working directory ancestor" " has no effect")
                )
            elif pas == [p1]:
                if not mergeancestor and wc.branch() == p2.branch():
                    raise error.Abort(
                        _("nothing to merge"),
                        hint=_("use '@prog@ goto' " "or check '@prog@ heads'"),
                    )
            if not force and (wc.files() or wc.deleted()):
                raise error.Abort(
                    _("uncommitted changes"),
                    hint=_("use '@prog@ status' to list changes"),
                )

        elif not overwrite:
            if p1 == p2:  # no-op update
                # call the hooks and exit early
                repo.hook("preupdate", throw=True, parent1=xp2, parent2="")
                repo.hook("update", parent1=xp2, parent2="", error=0)
                return 0, 0, 0, 0

            if updatecheck == "linear" and pas not in ([p1], [p2]):  # nonlinear
                dirty = wc.dirty(missing=True)
                if dirty:
                    # Branching is a bit strange to ensure we do the minimal
                    # amount of call to mutation.foreground_contains.
                    if mutation.enabled(repo):
                        in_foreground = mutation.foreground_contains(
                            repo, [p1.node()], repo[node].node()
                        )
                    else:
                        in_foreground = False
                    # note: the <node> variable contains a random identifier
                    if in_foreground:
                        pass  # allow updating to successors
                    else:
                        msg = _("uncommitted changes")
                        hint = _("commit or goto --clean to discard changes")
                        raise error.UpdateAbort(msg, hint=hint)
                else:
                    # Allow jumping branches if clean and specific rev given
                    pass

        if overwrite:
            pas = [wc]
        elif not branchmerge:
            pas = [p1]

        # deprecated config: merge.followcopies
        followcopies = repo.ui.configbool("merge", "followcopies")
        if overwrite:
            followcopies = False
        elif not pas[0]:
            followcopies = False
        if not branchmerge and not wc.dirty(missing=True):
            followcopies = False

        ### calculate phase
        with progress.spinner(repo.ui, "calculating"):
            actionbyfile, diverge, renamedelete = calculateupdates(
                repo,
                wc,
                p2,
                pas,
                branchmerge,
                force,
                mergeancestor,
                followcopies,
            )

        if updatecheck == "noconflict":
            paths = []
            cwd = repo.getcwd()
            for f, (m, args, msg) in pycompat.iteritems(actionbyfile):
                if m not in ("g", "k", "e", "r", "rg", "pr"):
                    paths.append(repo.pathto(f, cwd))

            paths = sorted(paths)
            if len(paths) > 0:
                msg = _("%d conflicting file changes:\n") % len(paths)
                for path in i18n.limititems(paths):
                    msg += " %s\n" % path
                hint = _(
                    "commit, shelve, goto --clean to discard all your changes"
                    ", or update --merge to merge them"
                )
                raise error.Abort(msg.strip(), hint=hint)

        # Convert to dictionary-of-lists format
        actions = dict((m, []) for m in "a am f g cd dc r rg dm dg m e k p pr".split())
        for f, (m, args, msg) in pycompat.iteritems(actionbyfile):
            if m not in actions:
                actions[m] = []
            actions[m].append((f, args, msg))

        # divergent renames
        for f, fl in sorted(pycompat.iteritems(diverge)):
            repo.ui.warn(
                _("note: possible conflict - %s was renamed " "multiple times to:\n")
                % f
            )
            for nf in fl:
                repo.ui.warn(" %s\n" % nf)

        # rename and delete
        for f, fl in sorted(pycompat.iteritems(renamedelete)):
            repo.ui.warn(
                _("note: possible conflict - %s was deleted " "and renamed to:\n") % f
            )
            for nf in fl:
                repo.ui.warn(" %s\n" % nf)

        ### apply phase
        if not branchmerge:  # just jump to the new rev
            fp1, fp2, xp1, xp2 = fp2, nullid, xp2, ""
        if not wc.isinmemory():
            repo.hook("preupdate", throw=True, parent1=xp1, parent2=xp2)
            # note that we're in the middle of an update
            repo.localvfs.writeutf8("updatestate", p2.hex())

        # Advertise fsmonitor when its presence could be useful.
        #
        # We only advertise when performing an update from an empty working
        # directory. This typically only occurs during initial clone.
        #
        # We give users a mechanism to disable the warning in case it is
        # annoying.
        #
        # We only allow on Linux and MacOS because that's where fsmonitor is
        # considered stable.
        fsmonitorwarning = repo.ui.configbool("fsmonitor", "warn_when_unused")
        fsmonitorthreshold = repo.ui.configint("fsmonitor", "warn_update_file_count")
        try:
            extensions.find("fsmonitor")
            fsmonitorenabled = repo.ui.config("fsmonitor", "mode") != "off"
            # We intentionally don't look at whether fsmonitor has disabled
            # itself because a) fsmonitor may have already printed a warning
            # b) we only care about the config state here.
        except KeyError:
            fsmonitorenabled = False

        if (
            fsmonitorwarning
            and not fsmonitorenabled
            and p1.node() == nullid
            and len(actions["g"]) >= fsmonitorthreshold
            and pycompat.sysplatform.startswith(("linux", "darwin"))
        ):
            repo.ui.warn(
                _(
                    "(warning: large working directory being used without "
                    "fsmonitor enabled; enable fsmonitor to improve performance; "
                    'see "hg help -e fsmonitor")\n'
                )
            )

        stats = applyupdates(
            repo, actions, wc, p2, overwrite, labels=labels, ancestors=pas
        )

        if not wc.isinmemory():
            with repo.dirstate.parentchange():
                repo.setparents(fp1, fp2)
                recordupdates(repo, actions, branchmerge)
                # update completed, clear state
                util.unlink(repo.localvfs.join("updatestate"))

                # After recordupdates has finished, the checkout is considered
                # finished and we should persist the sparse profile config
                # changes.
                #
                # Ideally this would be part of some wider transaction framework
                # that ensures these things all happen atomically, but that
                # doesn't exist for the dirstate right now.
                if hasattr(repo, "_persistprofileconfigs"):
                    repo._persistprofileconfigs()

                if not branchmerge:
                    repo.dirstate.setbranch(p2.branch())

    if git.isgitformat(repo) and not wc.isinmemory():
        if branchmerge:
            ctx = p1
            mctx = p2
        else:
            ctx = p2
            mctx = None
        git.submodulecheckout(ctx, force=force, mctx=mctx)
    repo.hook("update", parent1=xp1, parent2=xp2, error=stats[3])

    # Log the number of files updated.
    repo.ui.log("update_size", update_filecount=sum(stats))
    postrecrawls = querywatchmanrecrawls(repo)
    repo.ui.log("watchman-recrawls", watchman_recrawls=postrecrawls - prerecrawls)

    return stats


def getsparsematchers(repo, fp1, fp2):
    shouldsparsematch = hasattr(repo, "sparsematch") and (
        "eden" not in repo.requirements or "edensparse" in repo.requirements
    )
    sparsematch = getattr(repo, "sparsematch", None) if shouldsparsematch else None
    if sparsematch is not None:
        from sapling.ext import sparse

        revs = {fp1, fp2}
        revs -= {nullid}

        repo._clearpendingprofileconfig(all=True)
        oldpatterns = sparse.getsparsepatterns(repo, fp1)
        oldmatcher = sparsematch(fp1)

        repo._creatependingprofileconfigs()

        newpatterns = sparse.getsparsepatterns(repo, fp2)
        newmatcher = sparsematch(fp2)

        # Ignore files that are not in either source or target sparse match
        # This is not enough if sparse profile changes, but works for checkout within same sparse profile
        matcher = sparsematch(*list(revs))

        # If sparse configs are identical, don't set old/new matchers.
        # This signals to nativecheckout that there isn't a sparse
        # profile transition.
        oldnewmatchers = None
        if not oldpatterns.equivalent(newpatterns):
            oldnewmatchers = (oldmatcher, newmatcher)

        # This can be optimized - if matchers are same, we can set sparsematchers = None
        # sparse.py does not do it, so we are not making things worse
        return matcher, oldnewmatchers
    else:
        return None, None


@util.timefunction("makenativecheckoutplan", 0, "ui")
def makenativecheckoutplan(repo, p1, p2, updateprogresspath=None):
    matcher, sparsematchers = getsparsematchers(repo, p1.node(), p2.node())

    return nativecheckout.checkoutplan(
        repo.ui._rcfg,
        repo.wvfs.base,
        p1.manifest(),
        p2.manifest(),
        matcher,
        sparsematchers,
        updateprogresspath,
    )


@util.timefunction("donativecheckout", 0, "ui")
def donativecheckout(repo, p1, p2, force, wc, prerecrawls):
    repo.ui.debug("Using native checkout\n")
    repo.ui.log(
        "nativecheckout",
        using_nativecheckout=True,
    )

    xp1 = str(p1)
    xp2 = str(p2)

    updateprogresspath = None
    if repo.ui.configbool("checkout", "resumable"):
        updateprogresspath = repo.localvfs.join("updateprogress")

    plan = makenativecheckoutplan(repo, p1, p2, updateprogresspath)

    if repo.ui.debugflag:
        repo.ui.debug("Native checkout plan:\n%s\n" % plan)

    if not force:
        status = nativestatus.status(repo.status(unknown=True))
        unknown = plan.check_unknown_files(
            p2.manifest(),
            repo.fileslog.filestore,
            repo.dirstate._map._tree,
            status,
        )
        if unknown:
            for f in unknown:
                repo.ui.warn(_("%s: untracked file differs\n") % f)

            raise error.Abort(
                _(
                    "untracked files in working directory "
                    "differ from files in requested revision"
                )
            )

        conflicts = plan.check_conflicts(status)
        if conflicts:
            msg = _("%d conflicting file changes:\n") % len(conflicts)
            msg += " " + "\n ".join(i18n.limititems(conflicts)) + "\n"
            hint = _(
                "commit, shelve, goto --clean to discard all your changes"
                ", or update --merge to merge them"
            )
            raise error.Abort(msg.strip(), hint=hint)

    # preserving checks as is, even though wc.isinmemory always false here
    if not wc.isinmemory():
        repo.hook("preupdate", throw=True, parent1=xp1, parent2=xp2)
        # note that we're in the middle of an update
        repo.localvfs.writeutf8("updatestate", p2.hex())

    fp1, fp2, xp1, xp2 = p2.node(), nullid, xp2, ""
    cwd = pycompat.getcwdsafe()

    repo.ui.debug("Applying to %s \n" % repo.wvfs.base)
    failed_removes = plan.apply(
        repo.fileslog.filestore,
    )
    for (path, err) in failed_removes:
        repo.ui.warn(_("update failed to remove %s: %s!\n") % (path, err))
    repo.ui.debug("Apply done\n")
    stats = plan.stats()

    if cwd and not pycompat.getcwdsafe():
        # cwd was removed in the course of removing files; print a helpful
        # warning.
        repo.ui.warn(
            _(
                "current directory was removed\n"
                "(consider changing to repo root: %s)\n"
            )
            % repo.root
        )

    if not wc.isinmemory():
        with repo.dirstate.parentchange():
            repo.setparents(fp1, fp2)
            plan.record_updates(repo.dirstate._map._tree)
            # update completed, clear state
            repo.localvfs.unlink("updatestate")
            repo.localvfs.unlink("updateprogress")

            # After recordupdates has finished, the checkout is considered
            # finished and we should persist the sparse profile config
            # changes.
            #
            # Ideally this would be part of some wider transaction framework
            # that ensures these things all happen atomically, but that
            # doesn't exist for the dirstate right now.
            if hasattr(repo, "_persistprofileconfigs"):
                repo._persistprofileconfigs()

    repo.hook("update", parent1=xp1, parent2=xp2, error=stats[3])
    postrecrawls = querywatchmanrecrawls(repo)
    repo.ui.log("watchman-recrawls", watchman_recrawls=postrecrawls - prerecrawls)
    return stats


def graft(repo, ctx, pctx, labels, keepparent=False):
    """Do a graft-like merge.

    This is a merge where the merge ancestor is chosen such that one
    or more changesets are grafted onto the current changeset. In
    addition to the merge, this fixes up the dirstate to include only
    a single parent (if keepparent is False) and tries to duplicate any
    renames/copies appropriately.

    ctx - changeset to rebase
    pctx - merge base, usually ctx.p1()
    labels - merge labels eg ['local', 'graft']
    keepparent - keep second parent if any

    """
    # If we're grafting a descendant onto an ancestor, be sure to pass
    # mergeancestor=True to update. This does two things: 1) allows the merge if
    # the destination is the same as the parent of the ctx (so we can use graft
    # to copy commits), and 2) informs update that the incoming changes are
    # newer than the destination so it doesn't prompt about "remote changed foo
    # which local deleted".
    mergeancestor = repo.changelog.isancestor(repo["."].node(), ctx.node())

    stats = merge(
        repo,
        ctx.node(),
        force=True,
        ancestor=pctx.node(),
        mergeancestor=mergeancestor,
        labels=labels,
    )

    pother = nullid
    parents = ctx.parents()
    if keepparent and len(parents) == 2 and pctx in parents:
        parents.remove(pctx)
        pother = parents[0].node()

    with repo.dirstate.parentchange():
        repo.setparents(repo["."].node(), pother)
        repo.dirstate.write(repo.currenttransaction())
        # fix up dirstate for copies and renames
        copies.duplicatecopies(repo, repo[None], ctx.rev(), pctx.rev())
    return stats


def _gethex(ctx):
    # for workingctx return p1 hex
    return ctx.hex() if ctx.node() and ctx.hex() != wdirhex else ctx.p1().hex()
