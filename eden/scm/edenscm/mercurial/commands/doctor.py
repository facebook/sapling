# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import contextlib
import os
import typing

from bindings import metalog, mutationstore, nodemap, revisionstore, tracing

from .. import hg, progress, revlog, util, vfs as vfsmod
from ..i18n import _
from ..node import nullid
from .cmdtable import command


# This command has to be norepo since loading a repo might just fail.
@command("doctor", norepo=True)
def doctor(ui, **opts):
    # type: (...) -> typing.Optional[int]
    """attempt to check and fix issues

    This command is still in its early days. So expect it does not fix all
    issues.
    """

    from .. import dispatch  # avoid cycle

    # Minimal logic to get key repo objects without actually constructing
    # a real repo object.
    repopath, ui = dispatch._getlocal(ui, "")
    if not repopath:
        runedenfsdoctor(ui)
        return
    repohgpath = os.path.join(repopath, ".hg")
    vfs = vfsmod.vfs(repohgpath)
    sharedhgpath = vfs.tryread("sharedpath") or repohgpath
    svfs = vfsmod.vfs(os.path.join(sharedhgpath, "store"))

    ui.write(_("checking internal storage\n"))
    if ui.configbool("mutation", "enabled"):
        repairsvfs(ui, svfs, "mutation", mutationstore.mutationstore)

    cl = repairchangelog(ui, svfs)
    if cl is None:
        # Lots of fixes depend on changelog.
        ui.write_err(_("changelog: cannot fix automatically (consider reclone)\n"))
        return 1

    if svfs.isdir("metalog"):
        repairsvfs(ui, svfs, "metalog", metalog.metalog)

    if svfs.isdir("allheads"):
        repairsvfs(ui, svfs, "allheads", nodemap.nodeset)

    # Construct the real repo object as shallowutil requires it.
    repo = hg.repository(ui, repopath)
    if "remotefilelog" in repo.requirements:
        from ...hgext.remotefilelog import shallowutil

        if ui.configbool("remotefilelog", "indexedlogdatastore"):
            path = shallowutil.getindexedlogdatastorepath(repo)
            repair(
                ui,
                "indexedlogdatastore",
                path,
                revisionstore.indexedlogdatastore.repair,
            )

        if ui.configbool("remotefilelog", "indexedloghistorystore"):
            path = shallowutil.getindexedloghistorystorepath(repo)
            repair(
                ui,
                "indexedloghistorystore",
                path,
                revisionstore.indexedloghistorystore.repair,
            )

    # Run eden doctor on an edenfs repo.
    if "eden" in repo.requirements:
        runedenfsdoctor(ui)


def repairsvfs(ui, svfs, name, fixobj):
    # type: (..., ..., str, ...) -> None
    """Attempt to repair path in repo.svfs"""
    repair(ui, name, svfs.join(name), fixobj.repair)


def repair(ui, name, path, fixfunc):
    # type: (..., str, str, ...) -> None
    """Attempt to repair path by using fixfunc"""
    with progress.spinner(ui, "checking %s" % name):
        oldmtime = mtime(path)
        try:
            message = fixfunc(path)
        except Exception as ex:
            ui.warn(_("%s: failed to fix: %s\n") % (name, ex))
        else:
            newmtime = mtime(path)
            tracing.singleton.event(
                (("cat", "repair"), ("name", "repair %s" % name), ("details", message))
            )
            if ui.verbose:
                ui.write_err(_("%s:\n %s\n") % (name, indent(message)))
            else:
                if oldmtime != newmtime:
                    ui.write_err(_("%s: repaired\n") % name)
                else:
                    ui.write_err(_("%s: looks okay\n") % name)


def quickchecklog(ui, log, name, knownbroken):
    """
    knownbroken: a set of known broken *changelog* revisions

    returns (rev, linkrev) of the first bad entry
    returns (None, None) if nothing is bad
    """
    lookback = 10
    rev = max(0, len(log) - lookback)
    numchecked = 0
    seengood = False
    with progress.bar(ui, _("checking %s") % name) as prog:
        while rev < len(log):
            numchecked += 1
            prog.value = (numchecked, rev)
            (startflags, clen, ulen, baserev, linkrev, p1, p2, node) = log.index[rev]
            if linkrev in knownbroken:
                ui.write(
                    _("%s: marked corrupted at rev %d (linkrev=%d)\n")
                    % (name, rev, linkrev)
                )
                return rev, linkrev
            try:
                log.revision(rev, raw=True)
                if rev != 0:
                    if (
                        startflags == 0
                        or linkrev == 0
                        or (p1 == 0 and p2 == 0)
                        or clen == 0
                        or ulen == 0
                        or node == nullid
                    ):
                        # In theory no 100% correct. But those fields being 0 is
                        # almost always a corruption practically.
                        raise ValueError("suspected bad revision data")
                seengood = True
                rev += 1
            except Exception:  #  RevlogError, mpatchError, ValueError, etc
                if rev == 0:
                    msg = _("all %s entries appear corrupt!") % (name,)
                    raise error.RevlogError(msg)
                if not seengood:
                    # If the earliest rev we looked at is bad, look back farther
                    lookback *= 2
                    rev = max(0, len(log) - lookback)
                    continue
                ui.write(
                    _("%s: corrupted at rev %d (linkrev=%d)\n") % (name, rev, linkrev)
                )
                return rev, linkrev
    ui.write(_("%s: looks okay\n") % name)
    return None, None


def truncate(ui, svfs, path, size, dryrun=True, backupprefix=""):
    oldsize = svfs.stat(path).st_size
    if oldsize == size:
        return
    if oldsize < size:
        ui.write(
            _("%s: bad truncation request: %s to %s bytes\n") % (path, oldsize, size)
        )
        return
    ui.write(_("truncating %s from %s to %s bytes\n") % (path, oldsize, size))
    if dryrun:
        return

    svfs.makedirs("truncate-backups")
    with svfs.open(path, "ab+") as f:
        f.seek(size)
        # backup the part being truncated
        backuppart = f.read(oldsize - size)
        if len(backuppart) != oldsize - size:
            raise error.Abort(_("truncate: cannot backup confidently"))
        with svfs.open(
            "truncate-backups/%s%s.backup-byte-%s-to-%s"
            % (backupprefix, svfs.basename(path), size, oldsize),
            "w",
        ) as bkf:
            bkf.write(backuppart)
        f.truncate(size)


def repairchangelog(ui, svfs):
    """Attempt to fix revlog-based chagnelog

    This function only fixes the common corruption issues where bad data is at
    the end of the revlog. It cannot fix or detect other non-trivial issues.
    """
    clname = "00changelog.i"
    try:
        cl = revlog.revlog(svfs, clname)
    except Exception:
        return None

    rev, linkrev = quickchecklog(ui, cl, "changelog", set())
    if rev is None:
        return cl
    if rev >= len(cl) or rev <= 0:
        return None

    datastart = cl.length(rev - 1) + cl.start(rev - 1)
    dryrun = False
    truncate(ui, svfs, clname, rev * 64, dryrun)
    truncate(ui, svfs, clname.replace(".i", ".d"), datastart, dryrun)
    ui.write_err(_("changelog: repaired\n"))
    cl = revlog.revlog(svfs, clname)
    return cl


def mtime(path):
    # type: (str) -> int
    """Return an integer that is likely changed if content of the directory is changed"""
    mtime = 0
    for dirpath, _dirnames, filenames in os.walk(path):
        paths = [os.path.join(path, dirpath, name) for name in filenames]
        mtime += sum(
            (st.st_mtime % 1024) + st.st_size * 1024
            for st in util.statfiles(paths)
            if st
        )
    return mtime


def indent(message):
    # type: (str) -> str
    return "".join(l and ("  %s" % l) or "\n" for l in message.splitlines(True)) + "\n"


def runedenfsdoctor(ui):
    ui.write(_("running 'edenfsctl doctor'\n"))
    os.system("edenfsctl doctor")
