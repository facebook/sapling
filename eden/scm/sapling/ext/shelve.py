# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shelve.py - save/restore working directory state using obsolescense
# markers

"""save and restore changes to the working directory

The "hg shelve" command saves changes made to the working directory
and reverts those changes, resetting the working directory to a clean
state.

Later on, the "hg unshelve" command restores the changes saved by "hg
shelve". Changes can be restored even after updating to a different
parent, in which case Mercurial's merge machinery will resolve any
conflicts if necessary.

You can have more than one shelved change outstanding at a time; each
shelved change has a distinct name. For details, see the help for "hg
shelve".
"""

import collections
import errno
import itertools
import os
import time
from typing import Optional

from sapling import (
    bookmarks,
    bundle2,
    bundlerepo,
    changegroup,
    cmdutil,
    error,
    exchange,
    hg,
    lock as lockmod,
    mdiff,
    merge,
    node as nodemod,
    patch,
    registrar,
    scmutil,
    templatefilters,
    util,
    vfs as vfsmod,
    visibility,
)
from sapling.i18n import _

from . import rebase


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-ext"

backupdir = "shelve-backup"
shelvedir = "shelved"
shelvefileextensions = ["hg", "patch", "oshelve"]
# universal extension is present in all types of shelves
patchextension = "patch"


class shelvedfile:
    """Helper for the file storing a single shelve

    Handles common functions on shelve files (.hg/.patch) using
    the vfs layer"""

    def __init__(self, repo, name, filetype=None):
        self.repo = repo
        self.name = name
        self.vfs = vfsmod.vfs(repo.localvfs.join(shelvedir))
        self.backupvfs = vfsmod.vfs(repo.localvfs.join(backupdir))
        self.ui = self.repo.ui
        if filetype:
            self.fname = name + "." + filetype
        else:
            self.fname = name

    def exists(self):
        return self.vfs.exists(self.fname)

    def filename(self):
        return self.vfs.join(self.fname)

    def backupfilename(self):
        def gennames(base):
            yield base
            base, ext = base.rsplit(".", 1)
            for i in itertools.count(1):
                yield "%s-%d.%s" % (base, i, ext)

        name = self.backupvfs.join(self.fname)
        for n in gennames(name):
            if not self.backupvfs.exists(n):
                return n

    def movetobackup(self):
        if not self.backupvfs.isdir():
            self.backupvfs.makedir()
        util.rename(self.filename(), self.backupfilename())

    def stat(self):
        return self.vfs.stat(self.fname)

    def opener(self, mode="rb"):
        try:
            return self.vfs(self.fname, mode)
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            raise error.Abort(_("shelved change '%s' not found") % self.name)

    def applybundle(self):
        fp = self.opener()
        try:
            gen = exchange.readbundle(self.repo.ui, fp, self.fname, self.vfs)
            bundle2.applybundle(
                self.repo,
                gen,
                self.repo.currenttransaction(),
                source="unshelve",
                url="bundle:" + self.vfs.join(self.fname),
            )
        finally:
            fp.close()

    def bundlerepo(self):
        return bundlerepo.bundlerepository(
            self.repo.baseui, self.repo.root, self.vfs.join(self.fname)
        )

    def writebundle(self, bases, node):
        cgversion = changegroup.safeversion(self.repo)
        if cgversion == "01":
            btype = "HG10BZ"
            compression = None
        else:
            btype = "HG20"
            compression = "BZ"

        cg = changegroup.changegroupsubset(
            self.repo, bases, [node], "shelve", version=cgversion
        )
        bundle2.writebundle(
            self.ui, cg, self.fname, btype, self.vfs, compression=compression
        )

    def writeobsshelveinfo(self, info):
        scmutil.simplekeyvaluefile(self.vfs, self.fname).write(info)

    def readobsshelveinfo(self):
        return scmutil.simplekeyvaluefile(self.vfs, self.fname).read()


class shelvedstate:
    """Handle persistence during unshelving operations.

    Handles saving and restoring a shelved state. Ensures that different
    versions of a shelved state are possible and handles them appropriately.
    """

    _version = 2
    _filename = "shelvedstate"
    _keep = "keep"
    _nokeep = "nokeep"
    # colon is essential to differentiate from a real bookmark name
    _noactivebook = ":no-active-bookmark"
    _obsbased = "obsbased"
    _traditional = "traditional"

    @classmethod
    def _parsenodelist(cls, s):
        if not s:
            return []
        return [nodemod.bin(h) for h in s.split(" ")]

    @classmethod
    def _verifyandtransform(cls, d):
        """Some basic shelvestate syntactic verification and transformation"""
        try:
            d["originalwctx"] = nodemod.bin(d["originalwctx"])
            d["pendingctx"] = nodemod.bin(d["pendingctx"])
            d["parents"] = cls._parsenodelist(d["parents"])
            d["nodestoremove"] = cls._parsenodelist(d["nodestoremove"])
        except (ValueError, TypeError, KeyError) as err:
            raise error.CorruptedState(str(err))

    @classmethod
    def _getversion(cls, repo):
        """Read version information from shelvestate file"""
        fp = repo.localvfs(cls._filename)
        try:
            version = int(fp.readline().strip())
        except ValueError as err:
            raise error.CorruptedState(str(err))
        finally:
            fp.close()
        return version

    @classmethod
    def load(cls, repo):
        version = cls._getversion(repo)
        if version != cls._version:
            raise error.Abort(_("unsupported shelve version: %s") % version)

        d = scmutil.simplekeyvaluefile(repo.localvfs, cls._filename).read(
            firstlinenonkeyval=True
        )

        cls._verifyandtransform(d)
        try:
            obj = cls()
            obj.name = d["name"]
            obj.wctx = repo[d["originalwctx"]]
            obj.pendingctx = repo[d["pendingctx"]]
            obj.parents = d["parents"]
            obj.nodestoremove = d["nodestoremove"]
            obj.branchtorestore = d.get("branchtorestore", "")
            obj.keep = d.get("keep") == cls._keep
            obj.activebookmark = ""
            if d.get("activebook", "") != cls._noactivebook:
                obj.activebookmark = d.get("activebook", "")
            obj.obsshelve = d["obsshelve"] == cls._obsbased
        except (error.RepoLookupError, KeyError) as err:
            raise error.CorruptedState(str(err))

        return obj

    @classmethod
    def save(
        cls,
        repo,
        name,
        originalwctx,
        pendingctx,
        nodestoremove,
        branchtorestore,
        keep=False,
        activebook="",
        obsshelve=False,
    ):
        info = {
            "name": name,
            "originalwctx": nodemod.hex(originalwctx.node()),
            "pendingctx": nodemod.hex(pendingctx.node()),
            "parents": " ".join([nodemod.hex(p) for p in repo.dirstate.parents()]),
            "nodestoremove": " ".join([nodemod.hex(n) for n in nodestoremove]),
            "branchtorestore": branchtorestore,
            "keep": cls._keep if keep else cls._nokeep,
            "activebook": activebook or cls._noactivebook,
            "obsshelve": cls._obsbased if obsshelve else cls._traditional,
        }
        scmutil.simplekeyvaluefile(repo.localvfs, cls._filename).write(
            info, firstline=str(cls._version)
        )

    @classmethod
    def clear(cls, repo):
        repo.localvfs.unlinkpath(cls._filename, ignoremissing=True)

    def removenodes(self, ui, repo):
        """Cleanup temporary nodes from the repo"""
        _hidenodes(repo, self.nodestoremove)


def cleanupoldbackups(repo) -> None:
    vfs = vfsmod.vfs(repo.localvfs.join(backupdir))
    maxbackups = repo.ui.configint("shelve", "maxbackups")
    hgfiles = [f for f in vfs.listdir() if f.endswith("." + patchextension)]
    hgfiles = sorted([(vfs.stat(f).st_mtime, f) for f in hgfiles])
    if 0 < maxbackups and maxbackups < len(hgfiles):
        bordermtime = hgfiles[-maxbackups][0]
    else:
        bordermtime = None
    for mtime, f in hgfiles[: len(hgfiles) - maxbackups]:
        if mtime == bordermtime:
            # keep it, because timestamp can't decide exact order of backups
            continue
        base = f[: -(1 + len(patchextension))]
        for ext in shelvefileextensions:
            vfs.tryunlink(base + "." + ext)


def _backupactivebookmark(repo):
    activebookmark = repo._activebookmark
    if activebookmark:
        bookmarks.deactivate(repo)
    return activebookmark


def _restoreactivebookmark(repo, mark) -> None:
    if mark:
        bookmarks.activate(repo, mark)


def _aborttransaction(repo) -> None:
    """Abort current transaction for shelve/unshelve, but keep dirstate"""
    tr = repo.currenttransaction()
    repo.dirstate.savebackup(tr, "dirstate.shelve")
    tr.abort()
    repo.dirstate.restorebackup(None, "dirstate.shelve")


def createcmd(ui, repo, pats, opts):
    """subcommand that creates a new shelve"""
    with repo.wlock():
        cmdutil.checkunfinished(repo)
        return _docreatecmd(ui, repo, pats, opts)


def getshelvename(repo, parent, opts):
    """Decide on the name this shelve is going to have"""

    def gennames():
        yield label
        for i in itertools.count(1):
            yield "%s-%02d" % (label, i)

    name = opts.get("name")
    label = repo._activebookmark or "default"
    # slashes aren't allowed in filenames, therefore we rename it
    label = label.replace("/", "_")
    label = label.replace("\\", "_")
    # filenames must not start with '.' as it should not be hidden
    if label.startswith("."):
        label = label.replace(".", "_", 1)

    if name:
        if shelvedfile(repo, name, patchextension).exists():
            e = _("a shelved change named '%s' already exists") % name
            raise error.Abort(e)

        # ensure we are not creating a subdirectory or a hidden file
        if "/" in name or "\\" in name:
            raise error.Abort(_("shelved change names can not contain slashes"))
        if name.startswith("."):
            raise error.Abort(_("shelved change names can not start with '.'"))

    else:
        for n in gennames():
            if not shelvedfile(repo, n, patchextension).exists():
                name = n
                break

    return name


def mutableancestors(ctx):
    """return all mutable ancestors for ctx (included)

    Much faster than the revset ancestors(ctx) & draft()"""
    seen = {nodemod.nullrev}
    visit = collections.deque()
    visit.append(ctx)
    while visit:
        ctx = visit.popleft()
        yield ctx.node()
        for parent in ctx.parents():
            rev = parent.rev()
            if rev not in seen:
                seen.add(rev)
                if parent.mutable():
                    visit.append(parent)


def getcommitfunc(extra, interactive, editor: bool = False):
    def commitfunc(ui, repo, message, match, opts):
        editor_ = False
        if editor:
            editor_ = cmdutil.getcommiteditor(editform="shelve.shelve", **opts)
        return repo.commit(
            message, ui.username(), opts.get("date"), match, editor=editor_, extra=extra
        )

    def interactivecommitfunc(ui, repo, *pats, **opts):
        match = scmutil.match(repo["."], pats, {})
        message = opts["message"]
        return commitfunc(ui, repo, message, match, opts)

    return interactivecommitfunc if interactive else commitfunc


def _nothingtoshelvemessaging(ui, repo, pats, opts) -> None:
    stat = repo.status(match=scmutil.match(repo[None], pats, opts))
    if stat.deleted:
        ui.status(
            _("nothing changed (%d missing files, see '@prog@ status')\n")
            % len(stat.deleted)
        )
    else:
        ui.status(_("nothing changed\n"))


def _shelvecreatedcommit(ui, repo, node, name) -> None:
    shelvedfile(repo, name, "oshelve").writeobsshelveinfo({"node": nodemod.hex(node)})
    cmdutil.export(
        repo,
        [node],
        fp=shelvedfile(repo, name, patchextension).opener("wb"),
        opts=mdiff.diffopts(git=True),
    )


def _includeunknownfiles(repo, pats, opts, extra) -> None:
    s = repo.status(match=scmutil.match(repo[None], pats, opts), unknown=True)
    if s.unknown:
        extra["shelve_unknown"] = "\0".join(s.unknown)
        repo[None].add(s.unknown)


def _docreatecmd(ui, repo, pats, opts) -> Optional[int]:
    wctx = repo[None]
    parents = wctx.parents()
    if len(parents) > 1:
        raise error.Abort(_("cannot shelve while merging"))
    parent = parents[0]

    if parent.node() != nodemod.nullid:
        desc = "shelve changes to: %s" % parent.description().split("\n", 1)[0]
    else:
        desc = "(changes in empty repository)"

    if not opts.get("message"):
        opts["message"] = desc

    activebookmark = None
    try:
        with repo.lock(), repo.transaction("commit", report=None):
            interactive = opts.get("interactive", False)
            includeunknown = opts.get("unknown", False) and not opts.get(
                "addremove", False
            )

            name = getshelvename(repo, parent, opts)
            activebookmark = _backupactivebookmark(repo)
            extra = {}
            if includeunknown:
                _includeunknownfiles(repo, pats, opts, extra)

            commitfunc = getcommitfunc(extra, interactive, editor=True)
            if not interactive:
                node = cmdutil.commit(ui, repo, commitfunc, pats, opts)
            else:
                node = cmdutil.dorecord(
                    ui,
                    repo,
                    commitfunc,
                    None,
                    False,
                    cmdutil.recordfilter,
                    *pats,
                    **opts,
                )
            if not node:
                _nothingtoshelvemessaging(ui, repo, pats, opts)
                return 1

            _hidenodes(repo, [node])
    except Exception:
        if activebookmark:
            bookmarks.activate(repo, activebookmark)
        raise

    _shelvecreatedcommit(ui, repo, node, name)

    if ui.formatted:
        desc = util.ellipsis(desc, ui.termwidth())
    ui.status(_("shelved as %s\n") % name)

    # current wc parent may be already obsolete because
    # it might have been created previously and shelve just
    # reuses it
    try:
        hg.update(repo, parent.node(), updatecheck="none")
    except Exception:
        # failed to update to the original revision, which has left us on the
        # (hidden) shelve commit.  Move directly to the original commit by
        # updating the dirstate parents.
        repo.setparents(parent.node())
        raise
    finally:
        if activebookmark:
            bookmarks.activate(repo, activebookmark)

    merge.try_conclude_merge_state(repo)


def _listshelvefileinfos(repo, shelvedir):
    """Return a list of (filename, type) pair"""
    # ignore the hidden attribute files created by MacOS:
    # https://fburl.com/7hc21dkc
    return [
        fileinfo
        for fileinfo in repo.localvfs.readdir(shelvedir)
        if not fileinfo[0].startswith("._")
    ]


def cleanupcmd(ui, repo) -> None:
    """subcommand that deletes all shelves"""
    with repo.wlock():
        for name, _type in _listshelvefileinfos(repo, shelvedir):
            suffix = name.rsplit(".", 1)[-1]
            if suffix in shelvefileextensions:
                shelvedfile(repo, name).movetobackup()
            cleanupoldbackups(repo)


def deletecmd(ui, repo, pats) -> None:
    """subcommand that deletes a specific shelve"""
    if not pats:
        raise error.Abort(_("no shelved changes specified!"))
    with repo.wlock():
        try:
            for name in pats:
                for suffix in shelvefileextensions:
                    shfile = shelvedfile(repo, name, suffix)
                    # patch file is necessary, as it should
                    # be present for any kind of shelve,
                    # but the .hg file is optional as in future we
                    # will add obsolete shelve with does not create a
                    # bundle
                    if shfile.exists() or suffix == patchextension:
                        shfile.movetobackup()
            cleanupoldbackups(repo)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            # pyre-fixme[61]: `name` is undefined, or not always defined.
            raise error.Abort(_("shelved change '%s' not found") % name)


def listshelves(repo):
    """return all shelves in repo as list of (time, filename)"""
    try:
        names = _listshelvefileinfos(repo, shelvedir)
    except OSError as err:
        if err.errno != errno.ENOENT:
            raise
        return []
    info = []
    for name, _type in names:
        pfx, sfx = name.rsplit(".", 1)
        if not pfx or sfx != patchextension:
            continue
        st = shelvedfile(repo, name).stat()
        info.append((st.st_mtime, shelvedfile(repo, pfx).filename()))
    return sorted(info, reverse=True)


def listcmd(ui, repo, pats, opts) -> None:
    """subcommand that displays the list of shelves"""
    pats = set(pats)
    width = 80
    if not ui.plain():
        width = ui.termwidth()
    namelabel = "shelve.newest"
    ui.pager("shelve")
    for mtime, name in listshelves(repo):
        sname = util.split(name)[1]
        if pats and sname not in pats:
            continue
        ui.write(sname, label=namelabel)
        namelabel = "shelve.name"
        if ui.quiet:
            ui.write("\n")
            continue
        ui.write(" " * (16 - len(sname)))
        used = 16
        age = "(%s)" % templatefilters.age(util.makedate(mtime), abbrev=True)
        ui.write(age, label="shelve.age")
        ui.write(" " * (12 - len(age)))
        used += 12
        with open(name + "." + patchextension, "rb") as fp:
            while True:
                line = fp.readline()
                if not line:
                    break
                if not line.startswith(b"#"):
                    desc = line.rstrip().decode()
                    if ui.formatted:
                        desc = util.ellipsis(desc, width - used)
                    ui.write(desc)
                    break
            ui.write("\n")
            if not (opts["patch"] or opts["stat"]):
                continue
            difflines = fp.readlines()
            if opts["patch"]:
                for chunk, label in patch.difflabel(iter, difflines):
                    ui.writebytes(chunk, label=label)
            if opts["stat"]:
                for chunk, label in patch.diffstatui(difflines, width=width):
                    ui.write(chunk, label=label)


def listshelvesfiles(repo):
    """return all shelves in the repo as list of filenames from the repo root"""
    return [util.split(filetuple[1])[1] for filetuple in listshelves(repo)]


def patchcmds(ui, repo, pats, opts, subcommand) -> None:
    """subcommand that displays shelves"""
    if len(pats) == 0:
        shelved = listshelves(repo)
        if len(shelved) < 1:
            raise error.Abort(_("no shelves found"))
        pats = [util.split(shelved[0][1])[1]]

    for shelfname in pats:
        if not shelvedfile(repo, shelfname, patchextension).exists():
            raise error.Abort(_("cannot find shelf %s") % shelfname)

    listcmd(ui, repo, pats, opts)


def checkparents(repo, state) -> None:
    """check parent while resuming an unshelve"""
    if state.parents != repo.dirstate.parents():
        raise error.Abort(_("working directory parents do not match unshelve state"))


def pathtofiles(repo, files):
    cwd = repo.getcwd()
    return [repo.pathto(f, cwd) for f in files]


def unshelveabort(ui, repo, state, opts) -> None:
    """subcommand that abort an in-progress unshelve"""
    with repo.lock():
        try:
            checkparents(repo, state)

            repo.localvfs.rename("unshelverebasestate", "rebasestate")
            try:
                rebase.rebase(ui, repo, **{"abort": True})
            except Exception:
                repo.localvfs.rename("rebasestate", "unshelverebasestate")
                raise

            mergefiles(ui, repo, state.wctx, state.pendingctx)
            state.removenodes(ui, repo)
        finally:
            shelvedstate.clear(repo)
            ui.warn(_("unshelve of '%s' aborted\n") % state.name)


def mergefiles(ui, repo, wctx, shelvectx) -> None:
    """updates to wctx and merges the changes from shelvectx into the
    dirstate."""
    with ui.configoverride({("ui", "quiet"): True}):
        hg.update(repo, wctx.node())
        files = []
        files.extend(shelvectx.files())
        files.extend(shelvectx.p1().files())

        # revert will overwrite unknown files, so move them out of the way
        for file in repo.status(unknown=True).unknown:
            if file in files:
                util.rename(
                    os.path.join(repo.root, file),
                    os.path.join(repo.root, scmutil.origpath(ui, repo, file)),
                )
        ui.pushbuffer(True)
        cmdutil.revert(
            ui,
            repo,
            shelvectx,
            repo.dirstate.parents(),
            *pathtofiles(repo, files),
            **{"no_backup": True},
        )
        ui.popbuffer()


def unshelvecleanup(ui, repo, name, opts) -> None:
    """remove related files after an unshelve"""
    if not opts.get("keep"):
        for filetype in shelvefileextensions:
            shfile = shelvedfile(repo, name, filetype)
            if shfile.exists():
                shfile.movetobackup()
        cleanupoldbackups(repo)
    # rebase currently incorrectly leaves rebasestate behind even
    # in successful cases, see D4696578 for details.
    util.unlinkpath(repo.localvfs.join("rebasestate"), ignoremissing=True)


def unshelvecontinue(ui, repo, state, opts) -> None:
    """subcommand to continue an in-progress unshelve"""
    # We're finishing off a merge. First parent is our original
    # parent, second is the temporary "fake" commit we're unshelving.
    with repo.lock():
        checkparents(repo, state)
        ms = merge.mergestate.read(repo)
        if [f for f in ms if ms[f] == "u"]:
            raise error.Abort(
                _("unresolved conflicts, can't continue"),
                hint=_("see '@prog@ resolve', then '@prog@ unshelve --continue'"),
            )

        repo.localvfs.rename("unshelverebasestate", "rebasestate")
        try:
            # if shelve is obs-based, we want rebase to be able
            # to create markers to already-obsoleted commits
            with ui.configoverride(
                {("experimental", "rebaseskipobsolete"): "off"}, "unshelve"
            ):
                rebase.rebase(ui, repo, **{"continue": True})
        except Exception:
            repo.localvfs.rename("rebasestate", "unshelverebasestate")
            raise

        shelvectx = repo["tip"]
        if not shelvectx in state.pendingctx.children():
            # rebase was a no-op, so it produced no child commit
            shelvectx = state.pendingctx
        else:
            # only strip the shelvectx if the rebase produced it
            state.nodestoremove.append(shelvectx.node())

        mergefiles(ui, repo, state.wctx, shelvectx)

        state.removenodes(ui, repo)
        _restoreactivebookmark(repo, state.activebookmark)
        shelvedstate.clear(repo)
        unshelvecleanup(ui, repo, state.name, opts)
        ui.status(_("unshelve of '%s' complete\n") % state.name)


def _commitworkingcopychanges(ui, repo, opts, tmpwctx):
    """Temporarily commit working copy changes before moving unshelve commit"""
    # Store pending changes in a commit and remember added in case a shelve
    # contains unknown files that are part of the pending change
    s = repo.status()
    addedbefore = frozenset(s.added)
    if not (s.modified or s.added or s.removed):
        return tmpwctx, addedbefore
    ui.status(
        _(
            "temporarily committing pending changes "
            "(restore with '@prog@ unshelve --abort')\n"
        )
    )
    commitfunc = getcommitfunc(extra=None, interactive=False, editor=False)
    tempopts = {}
    tempopts["message"] = "pending changes temporary commit"
    tempopts["date"] = opts.get("date")
    with ui.configoverride({("ui", "quiet"): True}):
        node = cmdutil.commit(ui, repo, commitfunc, [], tempopts)
    tmpwctx = repo[node]
    ui.debug(
        "temporary working copy commit: %s:%s\n" % (tmpwctx.rev(), nodemod.short(node))
    )
    return tmpwctx, addedbefore


def _unshelverestorecommit(ui, repo, basename):
    """Recreate commit in the repository during the unshelve"""
    with ui.configoverride({("ui", "quiet"): True}):
        md = shelvedfile(repo, basename, "oshelve").readobsshelveinfo()
        shelvenode = nodemod.bin(md["node"])
        try:
            shelvectx = repo[shelvenode]
        except error.RepoLookupError:
            m = _(
                "shelved node %s not found in repo\nIf you think this shelve "
                "should exist, try running '@prog@ import --no-commit .hg/shelved/%s.patch' "
                "from the root of the repository."
            )
            raise error.Abort(m % (md["node"], basename))
    return repo, shelvectx


def _rebaserestoredcommit(
    ui,
    repo,
    opts,
    tr,
    oldrawheads,
    basename,
    pctx,
    tmpwctx,
    shelvectx,
    branchtorestore,
    activebookmark,
):
    """Rebase restored commit from its original location to a destination"""
    # If the shelve is not immediately on top of the commit
    # we'll be merging with, rebase it to be on top.
    if tmpwctx.node() == shelvectx.p1().node():
        # shelvectx is immediately on top of the tmpwctx
        return shelvectx

    # we need a new commit extra every time we perform a rebase to ensure
    # that "nothing to rebase" does not happen with obs-based shelve
    # "nothing to rebase" means that tip does not point to a "successor"
    # commit after a rebase and we have no way to learn which commit
    # should be a "shelvectx". this is a dirty hack until we implement
    # some way to learn the results of rebase operation, other than
    # text output and return code
    def extrafn(ctx, extra):
        extra["unshelve_time"] = str(time.time())

    ui.status(_("rebasing shelved changes\n"))

    try:
        # we only want keep to be true if shelve is traditional, since
        # for obs-based shelve, rebase will also be obs-based and
        # markers created help us track the relationship between shelvectx
        # and its new version
        rebase.rebase(
            ui,
            repo,
            **{
                "rev": [shelvectx.hex()],
                "dest": [tmpwctx.hex()],
                "keep": False,
                "tool": opts.get("tool", ""),
                "extrafn": extrafn,
            },
        )
    except error.InterventionRequired:
        tr.close()
        newrawheads = repo.dageval(lambda: heads(all()))
        nodestoremove = repo.dageval(lambda: only(newrawheads, oldrawheads))

        shelvedstate.save(
            repo,
            basename,
            pctx,
            tmpwctx,
            nodestoremove,
            branchtorestore,
            opts.get("keep"),
            activebookmark,
        )

        repo.localvfs.rename("rebasestate", "unshelverebasestate")
        raise error.InterventionRequired(
            _(
                "unresolved conflicts (see '@prog@ resolve', then "
                "'@prog@ unshelve --continue')"
            )
        )

    # refresh ctx after rebase completes
    shelvectx = repo["tip"]

    children = repo.changelog.children(tmpwctx.node())
    if not shelvectx.node() in children:
        # rebase was a no-op, so it produced no child commit
        shelvectx = tmpwctx
    return shelvectx


def _forgetunknownfiles(repo, shelvectx, addedbefore) -> None:
    # Forget any files that were unknown before the shelve, unknown before
    # unshelve started, but are now added.
    shelveunknown = shelvectx.extra().get("shelve_unknown")
    if not shelveunknown:
        return
    shelveunknown = frozenset(shelveunknown.split("\0"))
    addedafter = frozenset(repo.status().added)
    toforget = (addedafter & shelveunknown) - addedbefore
    repo[None].forget(toforget)


def _finishunshelve(repo, tr, activebookmark) -> None:
    _restoreactivebookmark(repo, activebookmark)
    tr.close()
    return


def _checkunshelveuntrackedproblems(ui, repo, shelvectx) -> None:
    """Check potential problems which may result from working
    copy having untracked changes."""
    wcdeleted = set(repo.status().deleted)
    shelvetouched = set(shelvectx.files())
    intersection = wcdeleted.intersection(shelvetouched)
    if intersection:
        m = _("shelved change touches missing files")
        hint = _("run @prog@ status to see which files are missing")
        raise error.Abort(m, hint=hint)


def _hideredundantnodes(repo, tr, pctx, shelvectx, tmpwctx) -> None:
    # order is important in the list of [shelvectx, tmpwctx] below
    # some nodes may already be obsolete
    tohide = []
    if shelvectx != pctx:
        tohide.append(shelvectx)
    if tmpwctx not in (pctx, shelvectx):
        tohide.append(tmpwctx)
    _hidenodes(repo, [ctx.node() for ctx in tohide])


def _hidenodes(repo, nodes) -> None:
    if visibility.tracking(repo):
        visibility.remove(repo, nodes)


@command(
    "unshelve",
    [
        ("a", "abort", None, _("abort an incomplete unshelve operation")),
        ("c", "continue", None, _("continue an incomplete unshelve operation")),
        ("k", "keep", None, _("keep shelve after unshelving")),
        ("n", "name", "", _("restore shelved change with given name"), _("NAME")),
        ("t", "tool", "", _("specify merge tool")),
        ("", "date", "", _("set date for temporary commits (DEPRECATED)"), _("DATE")),
    ],
    _("@prog@ unshelve [[-n] SHELVED]"),
    legacyaliases=["unshe", "unshel", "unshelv"],
)
def unshelve(ui, repo, *shelved, **opts):
    """restore a shelved change to the working copy

    This command accepts an optional name of a shelved change to
    restore. If none is given, the most recent shelved change is used.

    If a shelved change is applied successfully, the bundle that
    contains the shelved changes is moved to a backup location
    (.@prog@/shelve-backup).

    Since you can restore a shelved change on top of an arbitrary
    commit, it is possible that unshelving will result in a conflict. If
    this occurs, you must resolve the conflict, then use ``--continue``
    to complete the unshelve operation. The bundle will not be moved
    until you successfully complete the unshelve.

    Alternatively, you can use ``--abort`` to cancel the conflict
    resolution and undo the unshelve, leaving the shelve bundle intact.

    After a successful unshelve, the shelved changes are stored in a
    backup directory. Only the N most recent backups are kept. N
    defaults to 10 but can be overridden using the ``shelve.maxbackups``
    configuration option.

    .. container:: verbose

       Timestamp in seconds is used to decide the order of backups. More
       than ``maxbackups`` backups are kept if same timestamp prevents
       from deciding exact order of them, for safety.

    Returns 0 on success.
    """
    with repo.wlock():
        return _dounshelve(ui, repo, *shelved, **opts)


def _dounshelve(ui, repo, *shelved, **opts):
    abortf = opts.get("abort")
    continuef = opts.get("continue")
    if not abortf and not continuef:
        cmdutil.checkunfinished(repo)
    shelved = list(shelved)
    if opts.get("name"):
        shelved.append(opts["name"])

    if abortf or continuef:
        if abortf and continuef:
            raise error.Abort(_("cannot use both abort and continue"))
        if shelved:
            raise error.Abort(
                _("cannot combine abort/continue with naming a shelved change")
            )
        if abortf and opts.get("tool", False):
            ui.warn(_("tool option will be ignored\n"))

        try:
            state = shelvedstate.load(repo)
            if opts.get("keep") is None:
                opts["keep"] = state.keep
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            cmdutil.wrongtooltocontinue(repo, _("unshelve"))
        except error.CorruptedState as err:
            ui.debug(str(err) + "\n")
            if continuef:
                msg = _("corrupted shelved state file")
                hint = _("please run hg unshelve --abort to abort unshelve operation")
                raise error.Abort(msg, hint=hint)
            elif abortf:
                msg = _(
                    "could not read shelved state file, your working copy "
                    "may be in an unexpected state\nplease update to some "
                    "commit\n"
                )
                ui.warn(msg)
                shelvedstate.clear(repo)
            return

        if abortf:
            return unshelveabort(ui, repo, state, opts)
        elif continuef:
            return unshelvecontinue(ui, repo, state, opts)
    elif len(shelved) > 1:
        raise error.Abort(_("can only unshelve one change at a time"))
    elif not shelved:
        shelved = listshelves(repo)
        if not shelved:
            raise error.Abort(_("no shelved changes to apply!"))
        basename = util.split(shelved[0][1])[1]
        ui.status(_("unshelving change '%s'\n") % basename)
    else:
        basename = shelved[0]

    if not shelvedfile(repo, basename, patchextension).exists():
        raise error.Abort(_("shelved change '%s' not found") % basename)

    lock = tr = None
    shelvedfile(repo, basename, "oshelve")
    try:
        lock = repo.lock()
        tr = repo.transaction("unshelve", report=lambda x: None)
        oldrawheads = repo.dageval(lambda: heads(all()))

        pctx = repo["."]
        tmpwctx = pctx
        # The goal is to have a commit structure like so:
        # ...-> pctx -> tmpwctx -> shelvectx
        # where tmpwctx is an optional commit with the user's pending changes
        # and shelvectx is the unshelved changes. Then we merge it all down
        # to the original pctx.

        activebookmark = _backupactivebookmark(repo)
        tmpwctx, addedbefore = _commitworkingcopychanges(ui, repo, opts, tmpwctx)
        repo, shelvectx = _unshelverestorecommit(ui, repo, basename)
        _checkunshelveuntrackedproblems(ui, repo, shelvectx)
        branchtorestore = ""

        rebaseconfigoverrides = {
            ("ui", "forcemerge"): opts.get("tool", ""),
            ("experimental", "rebaseskipobsolete"): "off",
        }
        with ui.configoverride(rebaseconfigoverrides, "unshelve"):
            shelvectx = _rebaserestoredcommit(
                ui,
                repo,
                opts,
                tr,
                oldrawheads,
                basename,
                pctx,
                tmpwctx,
                shelvectx,
                branchtorestore,
                activebookmark,
            )
            mergefiles(ui, repo, pctx, shelvectx)
            _forgetunknownfiles(repo, shelvectx, addedbefore)

        _hideredundantnodes(repo, tr, pctx, shelvectx, tmpwctx)

        shelvedstate.clear(repo)
        _finishunshelve(repo, tr, activebookmark)
        unshelvecleanup(ui, repo, basename, opts)
    finally:
        if tr:
            tr.release()
        lockmod.release(lock)


@command(
    "shelve",
    [
        (
            "A",
            "addremove",
            None,
            _("mark new/missing files as added/removed before shelving"),
        ),
        ("u", "unknown", None, _("store unknown files in the shelve")),
        ("", "cleanup", None, _("delete all shelved changes")),
        ("", "date", "", _("shelve with the specified commit date"), _("DATE")),
        ("d", "delete", None, _("delete the named shelved change(s)")),
        ("e", "edit", False, _("invoke editor on commit messages")),
        ("l", "list", None, _("list current shelves")),
        ("m", "message", "", _("use text as shelve message"), _("TEXT")),
        ("n", "name", "", _("use the given name for the shelved commit"), _("NAME")),
        ("p", "patch", None, _("show patch")),
        (
            "i",
            "interactive",
            None,
            _("interactive mode - only works while creating a shelve"),
        ),
        ("", "stat", None, _("output diffstat-style summary of changes")),
    ]
    + cmdutil.walkopts,
    _("@prog@ shelve [OPTION]... [FILE]..."),
    legacyaliases=["she", "shel", "shelv"],
)
def shelvecmd(ui, repo, *pats, **opts):
    """save pending changes and revert working copy to a clean state

    Shelving takes files that :prog:`status` reports as not clean, saves
    the modifications to a bundle (a shelved change), and reverts the
    files to a clean state in the working copy.

    To restore the changes to the working copy, use :prog:`unshelve`
    regardless of your current commit.

    When no files are specified, :prog:`shelve` saves all not-clean
    files. If specific files or directories are named, only changes to
    those files are shelved.

    Each shelved change has a name that makes it easier to find later.
    The name of a shelved change by default is based on the active
    bookmark. To specify a different name, use ``--name``.

    To see a list of existing shelved changes, use the ``--list``
    option. For each shelved change, this will print its name, age,
    and description. Use ``--patch`` or ``--stat`` for more details.

    To delete specific shelved changes, use ``--delete``. To delete
    all shelved changes, use ``--cleanup``.

    Returns 0 on success.
    """
    allowables = [
        ("addremove", {"create"}),  # 'create' is pseudo action
        ("unknown", {"create"}),
        ("cleanup", {"cleanup"}),
        #       ('date', {'create'}), # ignored for passing '--date "0 0"' in tests
        ("delete", {"delete"}),
        ("edit", {"create"}),
        ("list", {"list"}),
        ("message", {"create"}),
        ("name", {"create"}),
        ("patch", {"patch", "list"}),
        ("stat", {"stat", "list"}),
    ]

    def checkopt(opt):
        if opts.get(opt):
            for i, allowable in allowables:
                if opts[i] and opt not in allowable:
                    raise error.Abort(
                        _("options '--%s' and '--%s' may not be used together")
                        % (opt, i)
                    )
            return True

    if checkopt("cleanup"):
        if pats:
            raise error.Abort(_("cannot specify names when using '--cleanup'"))
        return cleanupcmd(ui, repo)
    elif checkopt("delete"):
        return deletecmd(ui, repo, pats)
    elif checkopt("list"):
        return listcmd(ui, repo, pats, opts)
    elif checkopt("patch"):
        return patchcmds(ui, repo, pats, opts, subcommand="patch")
    elif checkopt("stat"):
        return patchcmds(ui, repo, pats, opts, subcommand="stat")
    else:
        return createcmd(ui, repo, pats, opts)


def extsetup(ui) -> None:
    cmdutil.afterresolvedstates.append(
        (shelvedstate._filename, _("@prog@ unshelve --continue"))
    )


revsetpredicate = registrar.revsetpredicate()


@revsetpredicate("shelved")
def shelved(repo, subset, x):
    """Shelved changes"""
    # list files with shelves
    shelves = [
        shelvedfile(repo, filename, "oshelve") for filename in listshelvesfiles(repo)
    ]
    # filter valid files
    shelves = filter(lambda f: f.exists(), shelves)
    # read node from each file
    nodes = [nodemod.bin(shelve.readobsshelveinfo()["node"]) for shelve in shelves]
    # filter if some of the revisions are not in repo
    # local=True because shelved commits cannot be public and only public
    # commits can be lazy so we avoid remote lookups.
    nodes = repo.changelog.filternodes(nodes, local=True)
    # returns intersection with shelved commits (including hidden)
    return subset & repo.revs("%ln", nodes)


templatekeyword = registrar.templatekeyword()


@templatekeyword("shelvename")
def shelvename(repo, ctx, templ, **args):
    """String.  The name of the shelved commit that this commit contains."""
    node = ctx.node()
    for filename in listshelvesfiles(repo):
        shelve = shelvedfile(repo, filename, "oshelve")
        if shelve.exists() and nodemod.bin(shelve.readobsshelveinfo()["node"]) == node:
            return shelve.name
    return ""
