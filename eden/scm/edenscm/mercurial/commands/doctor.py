# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import typing

from bindings import (
    dag,
    metalog,
    mutationstore,
    nodemap,
    repo as rsrepo,
    revisionstore,
    tracing,
    treestate as rawtreestate,
    zstore,
)

from .. import (
    bookmarks as bookmod,
    edenapi,
    error,
    extensions,
    hg,
    localrepo,
    progress,
    revlog,
    scmutil,
    treestate,
    util,
    vfs as vfsmod,
)
from ..i18n import _
from ..node import bin, hex, nullhex, nullid, short
from ..pycompat import decodeutf8, encodeutf8
from .cmdtable import command


# This command has to be norepo since loading a repo might just fail.
@command("doctor", norepo=True)
def doctor(ui, **opts) -> typing.Optional[int]:
    """attempt to check and fix issues

    Fix repo corruptions including:
    - changelog corruption at the end
    - dirstate pointing to an invalid commit
    - indexedlog corruptions (usually after hard reboot)
    """

    from .. import dispatch  # avoid cycle

    origui = ui

    # Minimal logic to get key repo objects without actually constructing
    # a real repo object.
    repopath, ui = dispatch._getlocal(origui, "")
    if not repopath:
        runglobalindexedlogdoctor(ui)
        runedenfsdoctor(ui)
        return

    repohgpath = os.path.join(repopath, ".hg")
    vfs = vfsmod.vfs(repohgpath)
    sharedhgpath = vfs.tryreadutf8("sharedpath").rstrip("\n") or repohgpath
    svfs = vfsmod.vfs(os.path.join(sharedhgpath, "store"))

    ui.write(_("checking internal storage\n"))

    ml = repairsvfs(ui, svfs, "metalog", metalog.metalog)
    # pyre-fixme[16]: `vfs` has no attribute `metalog`.
    svfs.metalog = ml

    if ui.configbool("mutation", "enabled"):
        repairsvfs(ui, svfs, "mutation", mutationstore.mutationstore)

    if svfs.exists("segments/v1"):
        repairsvfs(ui, svfs, "segments/v1", dag)
    if svfs.exists("hgcommits/v1"):
        repairsvfs(ui, svfs, "hgcommits/v1", zstore.zstore)

    repairrevlogchangelog(ui, svfs)
    cl = openchangelog(ui, svfs)
    if cl is None:
        # Lots of fixes depend on changelog.
        ui.write_err(_("changelog: cannot fix automatically (consider reclone)\n"))
        return 1

    repairvisibleheads(ui, ml, cl)
    repairtreestate(ui, vfs, repopath, cl)

    if svfs.isdir("allheads"):
        repairsvfs(ui, svfs, "allheads", nodemap.nodeset)

    # Construct the real repo object as shallowutil requires it.
    # User the original ui, so we don't load the repo on top of the ui we
    # manually loaded earlier. This caused unexpected side effects in the
    # dynamicconfig validation layer.
    repo = hg.repository(origui, repopath)
    ui = repo.ui
    if "remotefilelog" in repo.requirements:
        from ...hgext.remotefilelog import shallowutil

        sharedpath = os.path.join(shallowutil.getcachepath(ui), repo.name)
        localpath = svfs.base
        for suffix in [None, "manifests"]:
            repair(
                ui,
                "revisionstore",
                sharedpath,
                revisionstore.repair,
                localpath,
                suffix,
                ui._uiconfig._rcfg,
            )

    ui.write(_("checking commit references\n"))
    _try(ui, checkmissingmaster, repo)
    _try(ui, checklaggingremotename, repo)
    _try(ui, checktoomanynames, repo)
    _try(ui, checknoisybranches, repo)

    # Run eden doctor on an edenfs repo.
    if "eden" in repo.requirements:
        runedenfsdoctor(ui)


def repairsvfs(ui, svfs, name: str, fixobj) -> None:
    """Attempt to repair path in repo.svfs"""
    path = svfs.join(name)
    repair(ui, name, path, fixobj.repair)
    if callable(fixobj):
        return fixobj(path)


def repair(ui, name, path, fixfunc, *args, **kwargs):
    """Attempt to repair path by using fixfunc"""
    with progress.spinner(ui, "checking %s" % name):
        oldfshash = fshash(path)
        try:
            message = fixfunc(path, *args, **kwargs)
        except Exception as ex:
            ui.warn(_("%s: failed to fix: %s\n") % (name, ex))
        else:
            newfshash = fshash(path)
            tracing.singleton.event(
                (("cat", "repair"), ("name", "repair %s" % name), ("details", message))
            )
            if ui.verbose:
                ui.write_err(_("%s:\n%s\n") % (name, indent(message)))
            else:
                if oldfshash != newfshash:
                    ui.write_err(_("%s: repaired\n") % name)


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


def repairrevlogchangelog(ui, svfs):
    """Attempt to fix revlog-based chagnelog

    This function only fixes the common corruption issues where bad data is at
    the end of the revlog. It cannot fix or detect other non-trivial issues.
    """
    clname = "00changelog.i"
    try:
        cl = revlog.revlog(svfs, clname)
    except Exception:
        return None

    # Those two files are not necessary. Removing them forces rebuilding them.
    svfs.tryunlink("00changelog.len")
    svfs.tryunlink("00changelog.nodemap")

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


def openchangelog(ui, svfs):
    repo = ChangelogRepo(ui, svfs)
    try:
        cl = localrepo._openchangelog(repo)
    except Exception as e:
        if ui.debugflag or ui.tracebackflag:
            ui.traceback()
        ui.warn(_("cannot open changelog: %s\n") % e)
        return None
    return cl


def repairvisibleheads(ui, metalog, cl):
    """Attempt to fix visibleheads by removing invalid commit hashes"""
    oldtext = decodeutf8(metalog.get("visibleheads") or b"")
    oldlines = oldtext.splitlines()
    nodemap = cl.nodemap
    newlines = ["v1"] + [
        hexnode
        for hexnode in oldlines[1:]
        if len(hexnode) == 40 and bin(hexnode) in nodemap
    ]
    removedcount = max(len(oldlines), 1) - len(newlines)
    if removedcount or oldlines[:1] != ["v1"]:
        # Also add the "tip" node.
        hextip = hex(cl.tip())
        if hextip not in newlines:
            newlines.append(hextip)
        newtext = "".join(l + "\n" for l in newlines)
        metalog.set("visibleheads", encodeutf8(newtext))
        metalog.commit("fix visibleheads")
        ui.write_err(_("visibleheads: removed %s heads, added tip\n") % removedcount)


def repairtreestate(ui, vfs, root, cl):
    """Attempt to fix treestate:

    - Fix the dirstate pointer to a valid treestate root node.
    - Fix the dirstate node to point to a valid changelog node.
    """
    if not vfs.exists("treestate"):
        return
    needrebuild = False
    try:
        tmap = treestate.treestatemap(ui, vfs, root)
        p1node = tmap.p1()
        if p1node not in cl.nodemap:
            needrebuild = True
    except Exception:
        needrebuild = True
    if not needrebuild:
        return

    nodemap = cl.nodemap

    def stat(name):
        return vfs.stat("treestate/%s" % name)

    def rebuild(filename, rootpos, p1hex):
        meta = {"p1": p1hex, "filename": filename, "rootid": rootpos}
        p1bin = bin(p1hex)
        with vfs.open("dirstate", "w", atomictemp=True) as f:
            # see treestate.py:treestatemap.write
            f.write(p1bin)
            f.write(nullid)
            f.write(treestate.HEADER)
            f.write(treestate._packmetadata(meta))
        ui.write_err(_("treestate: repaired\n"))

    # Find a recent treestate (name, root) pair.
    for filename in sorted(vfs.listdir("treestate"), key=lambda n: -(stat(n).st_mtime)):
        data = vfs.read("treestate/%s" % filename)
        path = vfs.join("treestate/%s" % filename)

        end = len(data)
        while True:
            # Find the offset of "p1=".
            p1pos = data.rfind(b"p1=", 0, end)
            if p1pos < 0:
                break

            # Find a near root offset. A root offset has the property:
            # - It's before a "p1=" offset (if "p1=" is part of the root metadata,
            #   "p1=" can also be part of a filename or other things).
            # - It starts with "\0".
            # See treestate/src/serialization.rs for details.
            searchrange = 300
            end = max(p1pos, searchrange) - searchrange
            for rootpos in range(end, p1pos):
                # The first byte of the Root entry is "version", b"\0".
                # No need to try otherwise.
                if data[rootpos : rootpos + 1] != b"\0":
                    continue
                try:
                    rawtree = rawtreestate.treestate(path, rootpos)
                except Exception:
                    # Root deserialization failed xxhash check. Try next.
                    continue
                else:
                    meta = treestate._unpackmetadata(rawtree.getmetadata())
                    p1hex = meta.get("p1")
                    p2hex = meta.get("p2", nullhex)
                    if p2hex != nullhex:
                        # Do not restore to a merge commit.
                        continue
                    if p1hex is None or bin(p1hex) not in nodemap:
                        # Try next - p1 not in changelog.
                        continue
                    rebuild(filename, rootpos, p1hex)
                    return

    ui.write_err(
        _("treestate: cannot fix automatically (consider create a new workdir)\n")
    )


def _try(ui, func, *args, **kwargs):
    try:
        return func(*args, **kwargs)
    except Exception as ex:
        ui.warn(_("exception %r ignored during %s\n") % (ex, func.__name__))


def checkmissingmaster(repo, source="default"):
    ml = repo.metalog()
    ui = repo.ui
    if source not in ui.paths:
        # No such remote.
        return

    mainname = bookmod.mainbookmark(repo)
    # Respect remotenames mapping, "default/" -> "remote/"
    mainremote = ui.config("remotenames", "rename.%s" % source) or source
    mainfullname = "%s/%s" % (mainremote, mainname)

    namenodes = bookmod.decoderemotenames(ml["remotenames"])
    if mainfullname not in namenodes:
        # remote does not have the bookmark (in tests)
        return

    if mainfullname in repo:
        return

    ui.write_err(
        _("%s points to an unknown commit - trying to move it to a known commit\n")
        % mainfullname
    )

    # roots is sorted from old to new
    roots = ml.roots()
    found = None
    with progress.bar(ui, _("reading repo history"), "", len(roots)) as prog:
        for rootid in reversed(roots):
            prog.value += 1
            oldml = ml.checkout(rootid)
            node = bookmod.decoderemotenames(oldml["remotenames"]).get(mainfullname)
            if node and node in repo:
                found = node
                break

    if not found:
        ui.write_err(_("cannot find %s from repo history\n") % mainfullname)
        return

    ui.write_err(_("setting %s to %s\n") % (mainfullname, hex(found)))
    namenodes[mainfullname] = found
    ml["remotenames"] = bookmod.encoderemotenames(namenodes)
    ml.commit("doctor\nTransaction: checkmissingmaster\n")


def checklaggingremotename(repo, master=None, source="default"):
    """Check remote bookmarks:
    Pull selected bookmarks if they look like "lagging".
    """
    ui = repo.ui
    master = master or ui.config("doctor", "check-lag-name")
    if (
        not master
        or master not in bookmod.selectivepullbookmarknames(repo, source)
        or master not in repo
    ):
        return
    threshold = ui.configint("doctor", "check-lag-threshold")
    try:
        lag = len(repo.revs("limit(tip %% %s, %r)", master, str(threshold)))
    except Exception as ex:
        ui.write_err(_("check failed: %s\n") % ex)
        return
    if lag >= threshold:
        ui.write(_("%s might be lagging, running pull\n") % master)
        try:
            repo.pull(source, [master])
        except Exception as ex:
            ui.write_err(_("pull failed: %s\n") % ex)


def checktoomanynames(repo, source="default"):
    """Check if there are too many remotenames."""
    ui = repo.ui
    threshold = ui.configint("doctor", "check-too-many-names-threshold")
    namenodes = bookmod.decoderemotenames(repo.svfs.read("remotenames"))
    count = len(namenodes)
    selected = set(bookmod.selectivepullbookmarknames(repo, source))
    threshold += len(selected)
    if count < threshold:
        return
    defaultname = bookmod.remotenameforurl(ui, ui.paths.getpath(source).rawloc)
    if not defaultname:
        return
    ui.write(_("repo has too many (%s) remote bookmarks\n") % count)
    ui.write(
        _("(only %s of them (%s) are essential)\n")
        % (len(selected), ", ".join(sorted(selected)))
    )
    if (
        ui.promptchoice(_("only keep essential remote bookmarks (Yn)?$$ &Yes $$ &No"))
        != 0
    ):
        return
    with repo.wlock(), repo.lock(), repo.transaction("doctor"):
        newnamenodes = {}
        for fullname, node in namenodes.items():
            remotename, name = bookmod.splitremotename(fullname)
            if remotename == defaultname and name.strip() in selected:
                newnamenodes[fullname] = node
        repo.svfs.write("remotenames", bookmod.encoderemotenames(newnamenodes))


def checknoisybranches(repo):
    """Check draft branches that are likely not interesting and hide them.

    A draft branch (x) is not interesting if:
    - Not edited locally ('predecessors(x) - x' is empty).
    - Most (> 50%) changes are not authored by the current user.
    """
    ui = repo.ui
    commitcloudmod = extensions.find("commitcloud")
    if commitcloudmod:
        (
            current_workspace,
            locally_owned,
        ) = commitcloudmod.workspace.currentworkspacewithlocallyownedinfo(repo)
        if current_workspace:
            ui.write(
                _("checking irrelevant draft branches for the workspace '%s'\n")
                % current_workspace
            )
        if current_workspace and not locally_owned:
            ui.write(
                _(
                    "skipping draft branches check because the workspace '%s' doesn't belong to the current user\n"
                )
                % current_workspace
            )
            return
    heads = repo.changelog._visibleheads.heads
    noisyheads = set()
    for head in heads:
        x = list(repo.nodes("draft() & ::%n", head))
        predecessors = repo.revs("predecessors(%ln) - %ln", x, x)
        if predecessors:
            # The stack has local modifications. Skip it.
            continue
        authored = len(repo.revs("%ln & user(%s)", x, ui.username()))
        if authored * 2 >= len(x):
            # The stack is authored. Skip it.
            continue
        noisyheads.add(head)
    if not noisyheads:
        return
    ui.write(
        _("%s branches (%s) look less relevant\n")
        % (len(noisyheads), ", ".join(short(h) for h in noisyheads))
    )
    if ui.promptchoice(_("hide those branches (Yn)?$$ &Yes $$ &No")) != 0:
        return
    with repo.wlock(), repo.lock(), repo.transaction("doctor") as tr:
        heads = set(repo.changelog._visibleheads.heads)
        heads -= noisyheads
        repo.changelog._visibleheads.setvisibleheads(repo, heads, tr)


def fshash(path: str) -> int:
    """Return an integer that is likely changed if content of the directory is changed"""
    value = 0
    for dirpath, dirnames, filenames in os.walk(path):
        paths = [
            os.path.join(path, dirpath, name)
            for name in filenames + dirnames
            if name != "repair.log"
        ]
        value += len(paths)
        value += sum(
            (st.st_mtime % 1024) + st.st_size * 1024
            for st in util.statfiles(paths)
            if st
        )
    return value


def indent(message: str) -> str:
    return "".join(l and ("  %s" % l) or "\n" for l in message.splitlines(True)) + "\n"


def runedenfsdoctor(ui):
    useedendoctor = "HG_DOCTOR_SKIP_EDEN_DOCTOR" not in os.environ
    if useedendoctor:
        ui.write(_("running 'edenfsctl doctor'\n"))
        os.system("edenfsctl doctor")
    else:
        ui.write(
            _(
                "skipping running 'edenfsctl doctor' because "
                "HG_DOCTOR_SKIP_EDEN_DOCTOR is set. Our caller is likely "
                "'edenfsctl doctor'. \n"
            )
        )


def runglobalindexedlogdoctor(ui):
    """Global indexed log doctor"""
    if not ui.config("remotefilelog", "cachepath"):
        # remotefilelog is not enabled, skipping
        return

    from ...hgext.remotefilelog import shallowutil

    for path in shallowutil.getallcachepaths(ui):
        for suffix in [None, "manifests"]:
            repair(
                ui,
                "revisionstore",
                path,
                revisionstore.repair,
                None,
                suffix,
                ui._uiconfig._rcfg,
            )


class ChangelogRepo(object):
    """Minimal repo object to construct the changelog object"""

    def __init__(self, ui, svfs):
        shareddothgpath = os.path.dirname(svfs.join(""))
        sharedvfs = vfsmod.vfs(shareddothgpath)
        self.sharedvfs = sharedvfs
        root = os.path.dirname(shareddothgpath)
        ui.reloadconfigs(root)
        self.ui = ui
        self.svfs = svfs
        self.storerequirements = scmutil.readrequires(svfs)
        self._rsrepo = rsrepo.repo(root, self.ui._rcfg)

    @property
    def edenapi(self):
        return edenapi.getclient(self.ui)

    @property
    def name(self):
        return self.sharedvfs.tryreadutf8("reponame").strip()

    def metalog(self):
        return metalog.metalog(self.svfs.join("metalog"))
