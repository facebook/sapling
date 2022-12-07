# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# filemerge.py - file-level merge handling for Mercurial
#
# Copyright 2006, 2007, 2008 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import re
import subprocess
import tempfile

from . import (
    encoding,
    error,
    formatter,
    match,
    pathutil,
    pycompat,
    registrar,
    scmutil,
    simplemerge,
    templatefilters,
    templatekw,
    templater,
    util,
)
from .i18n import _
from .node import nullid, short


def _toolstr(ui, tool, part, *args):
    return ui.config("merge-tools", tool + "." + part, *args)


def _toolbool(ui, tool, part, *args):
    return ui.configbool("merge-tools", tool + "." + part, *args)


def _toollist(ui, tool, part):
    return ui.configlist("merge-tools", tool + "." + part)


internals = {}
# Merge tools to document.
internalsdoc = {}

internaltool = registrar.internalmerge()

# internal tool merge types
nomerge = internaltool.nomerge
mergeonly = internaltool.mergeonly  # just the full merge, no premerge
fullmerge = internaltool.fullmerge  # both premerge and merge

_localchangedotherdeletedmsg = _(
    "local%(l)s changed %(fd)s which other%(o)s deleted%(fa)s\n"
    "use (c)hanged version, (d)elete, or leave (u)nresolved?"
    "$$ &Changed $$ &Delete $$ &Unresolved"
)

_otherchangedlocaldeletedmsg = _(
    "other%(o)s changed %(fd)s which local%(l)s deleted\n"
    "use (c)hanged version, leave (d)eleted, "
    "leave (u)nresolved, or input (r)enamed path?"
    "$$ &Changed $$ &Deleted $$ &Unresolved $$ &Renamed"
)


class absentfilectx(object):
    """Represents a file that's ostensibly in a context but is actually not
    present in it.

    This is here because it's very specific to the filemerge code for now --
    other code is likely going to break with the values this returns."""

    def __init__(self, ctx, f):
        self._ctx = ctx
        self._f = f

    def path(self):
        return self._f

    def size(self):
        return None

    def data(self):
        return None

    def filenode(self):
        return nullid

    _customcmp = True

    def cmp(self, fctx):
        """compare with other file context

        returns True if different from fctx.
        """
        return not (
            fctx.isabsent() and fctx.ctx() == self.ctx() and fctx.path() == self.path()
        )

    def flags(self):
        return ""

    def changectx(self):
        return self._ctx

    def isbinary(self):
        return False

    def isabsent(self):
        return True


def _findtool(ui, repo, tool):
    if tool in internals:
        return tool
    return _findexternaltoolwithreporoot(ui, repo, tool)


def _findexternaltoolinternal(ui, tool):
    for kn in ("regkey", "regkeyalt"):
        k = _toolstr(ui, tool, kn)
        if not k:
            continue
        p = util.lookupreg(k, _toolstr(ui, tool, "regname"))
        if p:
            p = util.findexe(p + _toolstr(ui, tool, "regappend", ""))
            if p:
                return p
    return _toolstr(ui, tool, "executable", tool)


def findexternaltool(ui, tool):
    return util.findexe(util.expandpath(_findexternaltoolinternal(ui, tool)))


def _findexternaltoolwithreporoot(ui, repo, tool):
    """Like findexternaltool, but try checking inside the repo for the tool
    before looking in the path."""
    exe = _findexternaltoolinternal(ui, tool)
    if not os.path.isabs(exe):
        path = repo.wvfs.join(exe)
        if os.path.isfile(path) and util.isexec(path):
            return path
    return util.findexe(util.expandpath(exe))


def _picktool(repo, ui, path, binary, symlink, changedelete):
    def supportscd(tool):
        return tool in internals and internals[tool].mergetype == nomerge

    def check(tool, pat, symlink, binary, changedelete):
        tmsg = tool
        if pat:
            tmsg = _("%s (for pattern %s)") % (tool, pat)
        if not _findtool(ui, repo, tool):
            if pat:  # explicitly requested tool deserves a warning
                ui.warn(_("couldn't find merge tool %s\n") % tmsg)
            else:  # configured but non-existing tools are more silent
                ui.note(_("couldn't find merge tool %s\n") % tmsg)
        elif symlink and not _toolbool(ui, tool, "symlink"):
            ui.warn(_("tool %s can't handle symlinks\n") % tmsg)
        elif binary and not _toolbool(ui, tool, "binary"):
            ui.warn(_("tool %s can't handle binary\n") % tmsg)
        elif changedelete and not supportscd(tool):
            # the nomerge tools are the only tools that support change/delete
            # conflicts
            pass
        elif not util.gui() and _toolbool(ui, tool, "gui"):
            ui.warn(_("tool %s requires a GUI\n") % tmsg)
        else:
            return True
        return False

    # internal config: ui.forcemerge
    # forcemerge comes from command line arguments, highest priority
    force = ui.config("ui", "forcemerge")
    if force:
        toolpath = _findtool(ui, repo, force)
        if changedelete and not supportscd(toolpath):
            ui.debug("picktool() forcemerge :prompt\n")
            return ":prompt", None
        else:
            if toolpath:
                ui.debug("picktool() forcemerge toolpath %s\n" % toolpath)
                return (force, util.shellquote(toolpath))
            else:
                # mimic HGMERGE if given tool not found
                ui.debug("picktool() forcemerge toolpath not found %s\n" % force)
                return (force, force)

    # HGMERGE takes next precedence
    hgmerge = encoding.environ.get("HGMERGE")
    if hgmerge:
        if changedelete and not supportscd(hgmerge):
            ui.debug("picktool() hgmerge :prompt %s\n" % hgmerge)
            return ":prompt", None
        else:
            ui.debug("picktool() hgmerge %s\n" % hgmerge)
            return (hgmerge, hgmerge)

    # then patterns
    for pat, tool in ui.configitems("merge-patterns"):
        mf = match.match(repo.root, "", [pat])
        if mf(path) and check(tool, pat, symlink, False, changedelete):
            toolpath = _findtool(ui, repo, tool)
            ui.debug("picktool() merge-patterns tool=%s pat=%s\n" % (toolpath, pat))
            return (tool, util.shellquote(toolpath))

    # then merge tools
    tools = {}
    disabled = set()
    for k, v in ui.configitems("merge-tools"):
        t = k.split(".")[0]
        if t not in tools:
            tools[t] = int(_toolstr(ui, t, "priority"))
        if _toolbool(ui, t, "disabled"):
            disabled.add(t)
    names = tools.keys()
    tools = sorted([(-p, tool) for tool, p in tools.items() if tool not in disabled])
    plain = ui.plain(feature="mergetool")
    interactive = ui.interactive() and not plain
    ui.debug("picktool() interactive=%s plain=%s\n" % (ui.interactive(), plain))
    uimerge = None
    if interactive:
        uimerge = ui.config("ui", "merge:interactive")
    if not uimerge:
        uimerge = ui.config("ui", "merge")
    if uimerge:
        # external tools defined in uimerge won't be able to handle
        # change/delete conflicts
        if uimerge not in names and not changedelete:
            ui.debug(
                "picktool() uimerge merge:interactive=%s merge=%s\n"
                % (ui.config("ui", "merge:interactive"), ui.config("ui", "merge"))
            )
            return (uimerge, uimerge)
        tools.insert(0, (None, uimerge))  # highest priority
    tools.append((None, "hgmerge"))  # the old default, if found
    for p, t in tools:
        if check(t, None, symlink, binary, changedelete):
            toolpath = _findtool(ui, repo, t)
            ui.debug("picktool() tools\n")
            return (t, util.shellquote(toolpath))

    # internal merge or prompt as last resort
    if symlink or binary or changedelete:
        if not changedelete and len(tools):
            # any tool is rejected by capability for symlink or binary
            ui.warn(_("no tool found to merge %s\n") % path)
        return ":prompt", None
    ui.debug("picktool() :merge\n")
    return ":merge", None


def _eoltype(data):
    "Guess the EOL type of a file"
    if b"\0" in data:  # binary
        return None
    if b"\r\n" in data:  # Windows
        return b"\r\n"
    if b"\r" in data:  # Old Mac
        return b"\r"
    if b"\n" in data:  # UNIX
        return b"\n"
    return None  # unknown


def _matcheol(file, back):
    "Convert EOL markers in a file to match origfile"
    tostyle = _eoltype(back.data())  # No repo.wread filters?
    if tostyle:
        data = util.readfile(file)
        style = _eoltype(data)
        if style:
            newdata = data.replace(style, tostyle)
            if newdata != data:
                util.writefile(file, newdata)


@internaltool("prompt", nomerge)
def _iprompt(repo, mynode, orig, fcd, fco, fca, toolconf, labels=None):
    """Asks the user which of the local `p1()` or the other `p2()` version to
    keep as the merged version."""
    ui = repo.ui
    fd = fcd.path()
    fa = fca.path()

    # Avoid prompting during an in-memory merge since it doesn't support merge
    # conflicts.
    if fcd.changectx().isinmemory():
        raise error.InMemoryMergeConflictsError(
            "in-memory merge does not support file conflicts",
            type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
            paths=[fcd.path()],
        )

    prompts = partextras(labels)
    prompts["fd"] = fd
    prompts["fa"] = " (as %s)" % fa if fa != fd else ""

    try:
        if fco.isabsent():
            ui.metrics.gauge("filemerge_rename_otherdeleted", 1)
            index = ui.promptchoice(_localchangedotherdeletedmsg % prompts, 2)
            choice = ["local", "other", "unresolved"][index]
        elif fcd.isabsent():
            # Rebase example:
            #
            #   D (mynode)
            #   |
            #   B C (fco)      # B renames a file. C changes that file.
            #   |/             # run 'rebase -s C -d D' without copytracing
            #   A (fca)        # fcd: absent (working copy)
            #
            # Prompt looks like:
            #
            # other [source] changed <file-path> which local [dest] deleted
            # use (c)hanged version, leave (d)eleted, ...
            #
            # Here, 'other [source]' is a local draft commit, 'local [dest]'
            # is the rebase destination, a public commit.

            # Try to figure out "rename" path automatically by running
            # a script the user specified.
            # developer config: experimental.rename-cmd
            cmd = ui.config("experimental", "rename-cmd")
            if cmd:
                ui.status(
                    _("running %r to find rename destination of %s\n") % (cmd, fd)
                )
                try:
                    proc = subprocess.Popen(
                        cmd,
                        stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        shell=True,
                    )
                    destpath = pycompat.decodeutf8(
                        proc.communicate(fd.encode("utf-8").strip())[0]
                    ).strip()
                except Exception as ex:
                    ui.status(_(" failed: %s\n") % ex)
                else:
                    destctx = fcd.changectx()
                    if destpath in destctx:
                        ui.status(_(" trying rename destination: %s\n") % (destpath,))
                        fcd = destctx[destpath]
                        raise error.RetryFileMerge(fcd=fcd)
                    else:
                        ui.status(
                            _(" ignored invalid rename destination: %s\n") % (destpath,)
                        )

            ui.metrics.gauge("filemerge_prompt_localdeleted", 1)
            index = ui.promptchoice(_otherchangedlocaldeletedmsg % prompts, 2)
            choice = ["other", "local", "unresolved", "rename"][index]
        else:
            index = ui.promptchoice(
                _(
                    "keep (l)ocal%(l)s, take (o)ther%(o)s, or leave (u)nresolved"
                    " for %(fd)s?"
                    "$$ &Local $$ &Other $$ &Unresolved"
                )
                % prompts,
                2,
            )
            choice = ["local", "other", "unresolved"][index]

        if choice == "other":
            return _iother(repo, mynode, orig, fcd, fco, fca, toolconf, labels)
        elif choice == "local":
            return _ilocal(repo, mynode, orig, fcd, fco, fca, toolconf, labels)
        elif choice == "unresolved":
            return _ifail(repo, mynode, orig, fcd, fco, fca, toolconf, labels)
        elif choice == "rename":
            destctx = fcd.changectx()
            # fcd most likely a working copy that does not have a commit hash.
            # therefore 'mynode' is used for 'destnode', not fcd.node()
            destpath = ui.prompt(
                _(
                    "path '%(fd)s' in commit %(srcnode)s was renamed to [what path relative to repo root] in commit %(destnode)s ?"
                )
                % {"fd": fd, "srcnode": short(fco.node()), "destnode": short(mynode)},
                default="",
            )
            # normalize absolute path to repo path
            destpath = pathutil.canonpath(repo.root, repo.root, destpath)
            if not destpath or destpath not in destctx:
                ui.warn(
                    _(
                        "path '%(fd)s' does not exist in commit %(destnode)s - leave unresolved\n"
                    )
                    % {"fd": destpath, "destnode": short(mynode)}
                )
                return _ifail(repo, mynode, orig, fcd, fco, fca, toolconf, labels)
            else:
                fcd = destctx[destpath]
                raise error.RetryFileMerge(fcd=fcd)
    except error.ResponseExpected:
        ui.write("\n")
        return _ifail(repo, mynode, orig, fcd, fco, fca, toolconf, labels)


@internaltool("local", nomerge)
def _ilocal(repo, mynode, orig, fcd, fco, fca, toolconf, labels=None):
    """Uses the local `p1()` version of files as the merged version."""
    return 0, fcd.isabsent()


@internaltool("other", nomerge)
def _iother(repo, mynode, orig, fcd, fco, fca, toolconf, labels=None):
    """Uses the other `p2()` version of files as the merged version."""
    if fco.isabsent():
        # local changed, remote deleted -- 'deleted' picked
        _underlyingfctxifabsent(fcd).remove()
        deleted = True
    else:
        _underlyingfctxifabsent(fcd).write(fco.data(), fco.flags())
        deleted = False
    return 0, deleted


@internaltool("fail", nomerge)
def _ifail(repo, mynode, orig, fcd, fco, fca, toolconf, labels=None):
    """
    Rather than attempting to merge files that were modified on both
    branches, it marks them as unresolved. The resolve command must be
    used to resolve these conflicts."""
    # for change/delete conflicts write out the changed version, then fail
    if fcd.isabsent():
        _underlyingfctxifabsent(fcd).write(fco.data(), fco.flags())
    return 1, False


def _underlyingfctxifabsent(filectx):
    """Sometimes when resolving, our fcd is actually an absentfilectx, but
    we want to write to it (to do the resolve). This helper returns the
    underyling workingfilectx in that case.
    """
    if filectx.isabsent():
        return filectx.changectx()[filectx.path()]
    else:
        return filectx


def _premerge(repo, fcd, fco, fca, toolconf, files, labels=None):
    tool, toolpath, binary, symlink = toolconf
    if symlink or fcd.isabsent() or fco.isabsent():
        return 1
    unused, unused, unused, back = files

    ui = repo.ui

    validkeep = ["keep", "keep-merge3"]

    # do we attempt to simplemerge first?
    try:
        premerge = _toolbool(ui, tool, "premerge", not binary)
    except error.ConfigError:
        premerge = _toolstr(ui, tool, "premerge", "").lower()
        if premerge not in validkeep:
            _valid = ", ".join(["'" + v + "'" for v in validkeep])
            raise error.ConfigError(
                _("%s.premerge not valid " "('%s' is neither boolean nor %s)")
                % (tool, premerge, _valid)
            )

    if premerge:
        if premerge == "keep-merge3":
            if not labels:
                labels = _defaultconflictlabels
            if len(labels) < 3:
                labels.append("base")
        r = simplemerge.simplemerge(ui, fcd, fca, fco, quiet=True, label=labels)
        if not r:
            ui.debug(" premerge successful\n")
            return 0
        if premerge not in validkeep:
            # restore from backup and try again
            _restorebackup(fcd, back)
    return 1  # continue merging


def _mergecheck(repo, mynode, orig, fcd, fco, fca, toolconf):
    tool, toolpath, binary, symlink = toolconf
    if symlink:
        repo.ui.warn(
            _("warning: internal %s cannot merge symlinks " "for %s\n")
            % (tool, fcd.path())
        )
        return False
    if fcd.isabsent() or fco.isabsent():
        repo.ui.warn(
            _("warning: internal %s cannot merge change/delete " "conflict for %s\n")
            % (tool, fcd.path())
        )
        return False
    return True


def _findconflictingcommits(repo, fcd, fca, maxcommits=10):
    """Attempts to find all of the commits on the destination that might've
    conflicted during the merge.

    This is a first attempt that merely finds commits that touched the file
    between the ancestor and destination commit.

    In the future, a textual analysis using linelog, to filter that list
    to changes that edited the conflicted area, would be better. This requires
    fast remotefilelog fetches from the server so we can construct the linelog
    relatively quickly -- today, SSH overhead makes this infeasible.

    Returns changectx[], max_hit?
    """
    # current (fcd might be workingfilectx)
    #  :
    # common ancestor (fca)
    candidates = []
    if (
        repo.ui.configbool("experimental", "pathhistory.find-merge-conflicts")
        and repo.changelog.algorithmbackend == "segments"
    ):
        # use pathhistory to find conflicts
        paths = [fcd.path()]
        dnode = fcd.node()
        if dnode is None:
            dnode = fcd.changectx().p1().node()
        if dnode:
            anode = fca.node()
            nodes = repo.dageval(lambda: only([dnode], anode and [anode] or []))
            history = repo.pathhistory(paths, nodes)
            for node in history:
                candidates.append(repo[node])
                if len(candidates) >= maxcommits:
                    break
    else:
        # use linkrev to find conflicts (crashes with git)
        # (In a remotefilelog repo this will trigger a loose file download -- max
        # one per call to _find_conflicting_commits)
        # fcd is often workingfilectx, but not always.
        current = fcd.parents()[:1]
        while current:
            ctx = current[0].changectx()

            # Stop once we go beyond the common ancestor.
            if repo.changelog.isancestor(ctx.node(), fca.node()):
                break

            candidates.append(ctx)
            if len(candidates) >= maxcommits:
                break

            # Ignoring p2 path for simplicity. Ideally, we also follow p2, and
            # can draw the graph DAG to the user.
            current = current[0].parents()[:1]

    return candidates, len(candidates) >= maxcommits


def _describefailure(r, repo, mynode, orig, fcd, fco, fca):
    """Describes a merge conflict, which is rendered as a warning to the user"""

    relpath = repo.pathto(fcd.path())
    msg = _(
        "warning: %d conflicts while merging %s! (edit, then use '@prog@ resolve --mark')\n"
    ) % (r, relpath)

    if repo.ui.configbool("merge", "printcandidatecommmits"):
        candidates, hit_max = _findconflictingcommits(repo, fcd, fca)
    else:
        candidates, hit_max = [], False

    if len(candidates):
        msg += _(" %d commits might have introduced this conflict:\n") % (
            len(candidates),
        )
        for ctx in candidates:
            # TODO(phillco): Make this a configurable template
            firstline = templatefilters.firstline(ctx.description())
            msg += "  - [%s] %s\n" % (short(ctx.node()), firstline)
        if hit_max:
            msg += "  - (possibly more)\n"
    return msg


def _merge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels, mode):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Markers will have two sections, one for each side
    of merge, unless mode equals 'union' which suppresses the markers."""
    ui = repo.ui

    r = simplemerge.simplemerge(ui, fcd, fca, fco, label=labels, mode=mode)
    return True, r, False


@internaltool("union", fullmerge, _describefailure, precheck=_mergecheck)
def _iunion(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will use both left and right sides for conflict regions.
    No markers are inserted."""
    return _merge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels, "union")


@internaltool("merge", fullmerge, _describefailure, precheck=_mergecheck)
def _imerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Markers will have two sections, one for each side
    of merge."""
    return _merge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels, "merge")


@internaltool("merge3", fullmerge, _describefailure, precheck=_mergecheck)
def _imerge3(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Marker will have three sections, one from each
    side of the merge and one for the base content."""
    if not labels:
        labels = _defaultconflictlabels
    if len(labels) < 3:
        labels.append("base")
    return _imerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels)


@internaltool(
    "mergediff",
    fullmerge,
    _(
        "warning: conflicts while merging %s! "
        "(edit, then use '@prog@ resolve --mark')\n"
    ),
    precheck=_mergecheck,
)
def _imergediff(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. The marker will have two sections, one with the
    content from one side of the merge, and one with a diff from the base
    content to the content on the other side. (experimental)"""
    if not labels:
        labels = _defaultconflictlabels
    if len(labels) < 3:
        labels = labels + ["base"]
    return _merge(
        repo, mynode, orig, fcd, fco, fca, toolconf, files, labels, "mergediff"
    )


def _imergeauto(
    repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None, localorother=None
):
    """
    Generic driver for _imergelocal and _imergeother
    """
    assert localorother is not None
    tool, toolpath, binary, symlink = toolconf
    r = simplemerge.simplemerge(
        repo.ui, fcd, fca, fco, label=labels, localorother=localorother
    )
    return True, r


@internaltool("merge-local", mergeonly, precheck=_mergecheck)
def _imergelocal(*args, **kwargs):
    """
    Like :merge, but resolve all conflicts non-interactively in favor
    of the local `p1()` changes."""
    success, status = _imergeauto(localorother="local", *args, **kwargs)
    return success, status, False


@internaltool("merge-other", mergeonly, precheck=_mergecheck)
def _imergeother(*args, **kwargs):
    """
    Like :merge, but resolve all conflicts non-interactively in favor
    of the other `p2()` changes."""
    success, status = _imergeauto(localorother="other", *args, **kwargs)
    return success, status, False


@internaltool("dump", fullmerge)
def _idump(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Creates three versions of the files to merge, containing the
    contents of local, other and base. These files can then be used to
    perform a merge manually. If the file to be merged is named
    ``a.txt``, these files will accordingly be named ``a.txt.local``,
    ``a.txt.other`` and ``a.txt.base`` and they will be placed in the
    same directory as ``a.txt``.

    This implies premerge. Therefore, files aren't dumped, if premerge
    runs successfully. Use :forcedump to forcibly write files out.
    """
    a = _workingpath(repo, fcd)
    fd = fcd.path()

    from . import context

    if isinstance(fcd, context.overlayworkingfilectx):
        raise error.InMemoryMergeConflictsError(
            "in-memory merge does not support the :dump tool.",
            type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
            paths=[fcd.path()],
        )

    util.writefile(a + ".local", fcd.decodeddata())
    repo.wwrite(fd + ".other", fco.data(), fco.flags())
    repo.wwrite(fd + ".base", fca.data(), fca.flags())
    return False, 1, False


@internaltool("forcedump", mergeonly)
def _forcedump(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Creates three versions of the files as same as :dump, but omits premerge.
    """
    return _idump(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=labels)


def _xmergeimm(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    # In-memory merge simply raises an exception on all external merge tools,
    # for now.
    #
    # It would be possible to run most tools with temporary files, but this
    # raises the question of what to do if the user only partially resolves the
    # file -- we can't leave a merge state. (Copy to somewhere in the .hg/
    # directory and tell the user how to get it is my best idea, but it's
    # clunky.)
    raise error.InMemoryMergeConflictsError(
        "in-memory merge does not support external merge tools",
        type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
        paths=[fcd.path()],
    )


def _xmerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    tool, toolpath, binary, symlink = toolconf
    if fcd.isabsent() or fco.isabsent():
        repo.ui.warn(
            _("warning: %s cannot merge change/delete conflict " "for %s\n")
            % (tool, fcd.path())
        )
        return False, 1, None
    unused, unused, unused, back = files
    a = _workingpath(repo, fcd)
    b, c = _maketempfiles(repo, fco, fca)
    try:
        out = ""
        env = {
            "HGEDITOR": repo.ui.geteditor(),
            "HG_FILE": fcd.path(),
            "HG_MY_NODE": short(mynode),
            "HG_OTHER_NODE": str(fco.changectx()),
            "HG_BASE_NODE": str(fca.changectx()),
            "HG_MY_ISLINK": "l" in fcd.flags(),
            "HG_OTHER_ISLINK": "l" in fco.flags(),
            "HG_BASE_ISLINK": "l" in fca.flags(),
        }
        ui = repo.ui

        args = _toolstr(ui, tool, "args")
        if "$output" in args:
            # read input from backup, write to original
            out = a
            a = repo.wvfs.join(back.path())
        replace = {"local": a, "base": b, "other": c, "output": out}
        args = util.interpolate(
            r"\$", replace, args, lambda s: util.shellquote(util.localpath(s))
        )
        cmd = toolpath + " " + args
        if _toolbool(ui, tool, "gui"):
            repo.ui.status(
                _("running merge tool %s for file %s\n") % (tool, fcd.path())
            )
        repo.ui.debug("launching merge tool: %s\n" % cmd)
        r = ui.system(cmd, cwd=repo.root, environ=env, blockedtag="mergetool")
        repo.ui.debug("merge tool returned: %d\n" % r)
        return True, r, False
    finally:
        util.unlink(b)
        util.unlink(c)


def _formatconflictmarker(repo, ctx, template, label, pad):
    """Applies the given template to the ctx, prefixed by the label.

    Pad is the minimum width of the label prefix, so that multiple markers
    can have aligned templated parts.
    """
    if ctx.node() is None:
        ctx = ctx.p1()

    props = templatekw.keywords.copy()
    props["templ"] = template
    props["ctx"] = ctx
    props["repo"] = repo
    templateresult = template.render(props)

    label = ("%s:" % label).ljust(pad + 1)
    mark = "%s %s" % (label, templateresult)

    if mark:
        mark = mark.splitlines()[0]  # split for safety

    # 8 for the prefix of conflict marker lines (e.g. '<<<<<<< ')
    return util.ellipsis(mark, 80 - 8)


_defaultconflictlabels = ["local", "other"]


def _formatlabels(repo, fcd, fco, fca, labels):
    """Formats the given labels using the conflict marker template.

    Returns a list of formatted labels.
    """
    cd = fcd.changectx()
    co = fco.changectx()
    ca = fca.changectx()

    ui = repo.ui
    template = ui.config("ui", "mergemarkertemplate")
    template = templater.unquotestring(template)
    tmpl = formatter.maketemplater(ui, template)

    pad = max(len(l) for l in labels)

    newlabels = [
        _formatconflictmarker(repo, cd, tmpl, labels[0], pad),
        _formatconflictmarker(repo, co, tmpl, labels[1], pad),
    ]
    if len(labels) > 2:
        newlabels.append(_formatconflictmarker(repo, ca, tmpl, labels[2], pad))
    return newlabels


def partextras(labels):
    """Return a dictionary of extra labels for use in prompts to the user

    Intended use is in strings of the form "(l)ocal%(l)s".
    """
    if labels is None:
        return {"l": "", "o": ""}

    return {"l": " [%s]" % labels[0], "o": " [%s]" % labels[1]}


def _restorebackup(fcd, back):
    # TODO: Add a workingfilectx.write(otherfilectx) path so we can use
    # util.copy here instead.
    fcd.write(back.data(), fcd.flags())


def _makebackup(repo, ui, wctx, fcd, premerge):
    """Makes and returns a filectx-like object for ``fcd``'s backup file.

    In addition to preserving the user's pre-existing modifications to `fcd`
    (if any), the backup is used to undo certain premerges, confirm whether a
    merge changed anything, and determine what line endings the new file should
    have.

    Backups only need to be written once (right before the premerge) since their
    content doesn't change afterwards.
    """
    if fcd.isabsent():
        return None
    # TODO: Break this import cycle somehow. (filectx -> ctx -> fileset ->
    # merge -> filemerge). (I suspect the fileset import is the weakest link)
    from . import context

    a = _workingpath(repo, fcd)
    back = scmutil.origpath(ui, repo, a)
    inworkingdir = back.startswith(repo.wvfs.base) and not back.startswith(
        repo.localvfs.base
    )
    if isinstance(fcd, context.overlayworkingfilectx) and inworkingdir:
        # If the backup file is to be in the working directory, and we're
        # merging in-memory, we must redirect the backup to the memory context
        # so we don't disturb the working directory.
        relpath = back[len(repo.wvfs.base) + 1 :]
        if premerge:
            wctx[relpath].write(fcd.data(), fcd.flags())
        return wctx[relpath]
    else:
        if premerge:
            # Otherwise, write to wherever path the user specified the backups
            # should go. We still need to switch based on whether the source is
            # in-memory so we can use the fast path of ``util.copy`` if both are
            # on disk.
            if isinstance(fcd, context.overlayworkingfilectx):
                util.writefile(back, fcd.data())
            else:
                util.copyfile(a, back)
        # A arbitraryfilectx is returned, so we can run the same functions on
        # the backup context regardless of where it lives.
        return context.arbitraryfilectx(back, repo=repo)


def _maketempfiles(repo, fco, fca):
    """Writes out `fco` and `fca` as temporary files, so an external merge
    tool may use them.
    """

    def temp(prefix, ctx):
        fullbase, ext = os.path.splitext(ctx.path())
        pre = "%s~%s." % (os.path.basename(fullbase), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre, suffix=ext)
        data = repo.wwritedata(ctx.path(), ctx.data())
        f = util.fdopen(fd, "wb")
        f.write(data)
        f.close()
        return name

    b = temp("base", fca)
    c = temp("other", fco)

    return b, c


def _filemerge(premerge, repo, wctx, mynode, orig, fcd, fco, fca, labels=None):
    """perform a 3-way merge in the working directory

    premerge = whether this is a premerge
    mynode = parent node before merge
    orig = original local filename before merge
    fco = other file context
    fca = ancestor file context
    fcd = local file context for current/destination file

    Returns whether the merge is complete, the return value of the merge, and
    a boolean indicating whether the file was deleted from disk."""

    if not fco.cmp(fcd):  # files ide gtical?
        return True, None, False

    ui = repo.ui
    fd = fcd.path()
    relorig = repo.pathto(orig)
    relfo = repo.pathto(fco.path())
    relfd = repo.pathto(fd)

    binary = fcd.isbinary() or fco.isbinary() or fca.isbinary()
    symlink = "l" in fcd.flags() + fco.flags()
    changedelete = fcd.isabsent() or fco.isabsent()
    tool, toolpath = _picktool(repo, ui, fd, binary, symlink, changedelete)
    if tool in internals and tool.startswith("internal:"):
        # normalize to new-style names (':merge' etc)
        tool = tool[len("internal") :]
    ui.debug(
        "picked tool '%s' for %s (binary %s symlink %s changedelete %s)\n"
        % (
            tool,
            fd,
            pycompat.bytestr(binary),
            pycompat.bytestr(symlink),
            pycompat.bytestr(changedelete),
        )
    )

    if tool in internals:
        func = internals[tool]
        mergetype = func.mergetype
        onfailure = func.onfailure
        precheck = func.precheck
    else:
        if wctx.isinmemory():
            func = _xmergeimm
        else:
            func = _xmerge
        mergetype = fullmerge
        onfailure = _("merging %s failed!\n")
        precheck = None

    toolconf = tool, toolpath, binary, symlink

    if mergetype == nomerge:
        r, deleted = func(repo, mynode, orig, fcd, fco, fca, toolconf, labels)
        return True, r, deleted

    if premerge:
        if orig != fco.path():
            ui.status(_("merging %s and %s to %s\n") % (relorig, relfo, relfd))
        else:
            ui.status(_("merging %s\n") % relfd)

    ui.debug("my %s other %s ancestor %s\n" % (fcd, fco, fca))

    def getfailuremsg(on_failure, num_conflicts):
        # on_failure can be a string or a function which we'll call.
        if callable(on_failure):
            return on_failure(num_conflicts, repo, mynode, orig, fcd, fco, fca)
        else:
            return on_failure % relfd

    if precheck and not precheck(repo, mynode, orig, fcd, fco, fca, toolconf):
        if onfailure:
            if wctx.isinmemory():
                raise error.InMemoryMergeConflictsError(
                    "in-memory merge does not support merge conflicts",
                    type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
                    paths=[fcd.path()],
                )
            ui.warn(getfailuremsg(onfailure, 1))
        return True, 1, False

    back = _makebackup(repo, ui, wctx, fcd, premerge)
    files = (None, None, None, back)
    r = 1
    try:
        markerstyle = ui.config("ui", "mergemarkers")
        if not labels:
            labels = _defaultconflictlabels
        if markerstyle != "basic":
            labels = _formatlabels(repo, fcd, fco, fca, labels)

        if premerge and mergetype == fullmerge:
            r = _premerge(repo, fcd, fco, fca, toolconf, files, labels=labels)
            # complete if premerge successful (r is 0)
            return not r, r, False

        needcheck, r, deleted = func(
            repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=labels
        )

        if needcheck:
            r = _check(repo, r, ui, tool, fcd, files)

        if r:
            if onfailure:
                if wctx.isinmemory():
                    raise error.InMemoryMergeConflictsError(
                        "in-memory merge does not support merge conflicts",
                        type=error.InMemoryMergeConflictsError.TYPE_FILE_CONFLICTS,
                        paths=[fcd.path()],
                    )
                ui.warn(getfailuremsg(onfailure, r))
            _onfilemergefailure(ui)

        return True, r, deleted
    finally:
        if not r and back is not None:
            back.remove()


def _haltmerge():
    msg = _("merge halted after failed merge (see @prog@ resolve)")
    raise error.InterventionRequired(msg)


def _onfilemergefailure(ui):
    action = ui.config("merge", "on-failure")
    if action == "prompt":
        msg = _(
            "continue merge operation [(y)es/(n)o/(a)lways]?"
            "$$ &Yes $$ &No $$ &Always"
        )
        choice = ui.promptchoice(msg, 0)
        if choice == 1:  # "no"
            _haltmerge()
        elif choice == 2:  # "always"
            ui.setconfig("merge", "on-failure", "continue", "user-prompt")
    elif action == "halt":
        _haltmerge()
    # default action is 'continue', in which case we neither prompt nor halt


def _check(repo, r, ui, tool, fcd, files):
    fd = fcd.path()
    unused, unused, unused, back = files

    if not r and (
        _toolbool(ui, tool, "checkconflicts")
        or "conflicts" in _toollist(ui, tool, "check")
    ):
        if re.search(b"^(<<<<<<< .*|=======|>>>>>>> .*)$", fcd.data(), re.MULTILINE):
            r = 1

    checked = False
    if "prompt" in _toollist(ui, tool, "check"):
        checked = True
        if ui.promptchoice(
            _("was merge of '%s' successful (yn)?" "$$ &Yes $$ &No") % fd, 1
        ):
            r = 1

    if (
        not r
        and not checked
        and (
            _toolbool(ui, tool, "checkchanged")
            or "changed" in _toollist(ui, tool, "check")
        )
    ):
        if back is not None and not fcd.cmp(back):
            if ui.promptchoice(
                _(
                    " output file %s appears unchanged\n"
                    "was merge successful (yn)?"
                    "$$ &Yes $$ &No"
                )
                % fd,
                1,
            ):
                r = 1

    if back is not None and _toolbool(ui, tool, "fixeol"):
        _matcheol(_workingpath(repo, fcd), back)

    return r


def _workingpath(repo, ctx):
    return repo.wjoin(ctx.path())


def premerge(repo, wctx, mynode, orig, fcd, fco, fca, labels=None):
    return _filemerge(True, repo, wctx, mynode, orig, fcd, fco, fca, labels=labels)


def filemerge(repo, wctx, mynode, orig, fcd, fco, fca, labels=None):
    return _filemerge(False, repo, wctx, mynode, orig, fcd, fco, fca, labels=labels)


def loadinternalmerge(ui, extname, registrarobj):
    """Load internal merge tool from specified registrarobj"""
    for name, func in registrarobj._table.items():
        fullname = ":" + name
        internals[fullname] = func
        internals["internal:" + name] = func
        internalsdoc[fullname] = func


# load built-in merge tools explicitly to setup internalsdoc
loadinternalmerge(None, None, internaltool)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = internals.values()
