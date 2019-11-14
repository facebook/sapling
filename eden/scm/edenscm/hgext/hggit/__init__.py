# git.py - git server bridge
#
# Copyright 2008 Scott Chacon <schacon at gmail dot com>
#   also some code (and help) borrowed from durin42
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

"""push and pull from a Git server

This extension lets you communicate (push and pull) with a Git server.
This way you can use Git hosting for your project or collaborate with a
project that is in Git.  A bridger of worlds, this plugin be.

Try hg clone git:// or hg clone git+ssh://

For more information and instructions, see :hg:`help git`
"""

# global modules
import os
import shutil
import warnings
from bisect import insort

# local modules
import compat
import gitrepo
import hgrepo
import overlay
import util
import verify
from bindings import nodemap as nodemapmod
from edenscm.mercurial import (
    bundlerepo,
    cmdutil,
    demandimport,
    discovery,
    extensions,
    help,
    hg,
    localrepo,
    manifest,
    phases,
    pycompat,
    revset,
    scmutil,
    templatekw,
    ui as hgui,
    util as hgutil,
)
from edenscm.mercurial.error import LookupError
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid
from git_handler import GitHandler


# Disable DeprecationWarning from newer dulwich since hggit also supports older
# dulwich.
warnings.filterwarnings(r"ignore", r"", DeprecationWarning, r"edenscm.hgext.hggit")

try:
    from edenscm.mercurial import exchange

    exchange.push  # existed in first iteration of this file
except (AttributeError, ImportError):
    # We only *use* the exchange module in hg 3.2+, so this is safe
    pass

try:
    from edenscm.mercurial import ignore

    ignore.readpats
    ignoremod = True
except (AttributeError, ImportError):
    # The ignore module disappeared in Mercurial 3.5
    ignoremod = False

baseset = set
try:
    baseset = revset.baseset
except AttributeError:
    # baseset was added in hg 3.0
    pass

demandimport.ignore.extend(["collections"])

__version__ = "0.8.10"

testedwith = (
    "2.8.2 2.9.2 3.0.2 3.1.2 3.2.4 3.3.3 3.4.2 3.5.2 3.6.3 3.7.3"
    "3.8.4 3.9.2 4.0.2 4.1.3 4.2.3 4.3.3 4.4"
)
buglink = "https://bitbucket.org/durin42/hg-git/issues"

cmdtable = {}
configtable = {}
try:
    from edenscm.mercurial import registrar

    command = registrar.command(cmdtable)
    configitem = registrar.configitem(configtable)
    compat.registerconfigs(configitem)

except (ImportError, AttributeError):
    command = cmdutil.command(cmdtable)

# support for `hg clone git://github.com/defunkt/facebox.git`
# also hg clone git+ssh://git@github.com/schacon/simplegit.git
for _scheme in util.gitschemes:
    hg.schemes[_scheme] = gitrepo

# support for `hg clone localgitrepo`
_oldlocal = hg.schemes["file"]

try:
    urlcls = hgutil.url
except AttributeError:

    class urlcls(object):
        def __init__(self, path):
            self.p = hgutil.drop_scheme("file", path)

        def localpath(self):
            return self.p


def _isgitdir(path):
    """True if the given file path is a git repo."""
    if os.path.exists(os.path.join(path, ".hg")):
        return False

    if os.path.exists(os.path.join(path, ".git")):
        # is full git repo
        return True

    if (
        os.path.exists(os.path.join(path, "HEAD"))
        and os.path.exists(os.path.join(path, "objects"))
        and os.path.exists(os.path.join(path, "refs"))
    ):
        # is bare git repo
        return True

    return False


def _local(path):
    p = urlcls(path).localpath()
    if _isgitdir(p):
        return gitrepo
    # detect git ssh urls (which mercurial thinks is a file-like path)
    if util.isgitsshuri(p):
        return gitrepo
    return _oldlocal(path)


hg.schemes["file"] = _local

# we need to wrap this so that git-like ssh paths are not prepended with a
# local filesystem path. ugh.
def _url(orig, path, **kwargs):
    # we'll test for 'git@' then use our heuristic method to determine if it's
    # a git uri
    if not (path.startswith(pycompat.ossep) and ":" in path):
        return orig(path, **kwargs)

    # the file path will be everything up until the last slash right before the
    # ':'
    lastsep = path.rindex(pycompat.ossep, None, path.index(":")) + 1
    gituri = path[lastsep:]

    if util.isgitsshuri(gituri):
        return orig(gituri, **kwargs)
    return orig(path, **kwargs)


extensions.wrapfunction(hgutil, "url", _url)


def _httpgitwrapper(orig):
    # we should probably test the connection but for now, we just keep it
    # simple and check for a url ending in '.git'
    def httpgitscheme(uri):
        if uri.endswith(".git"):
            return gitrepo

        # the http(s) scheme just returns the _peerlookup
        return orig

    return httpgitscheme


hg.schemes["https"] = _httpgitwrapper(hg.schemes["https"])
hg.schemes["http"] = _httpgitwrapper(hg.schemes["http"])

hgdefaultdest = hg.defaultdest


def defaultdest(source):
    for scheme in util.gitschemes:
        if source.startswith("%s://" % scheme) and source.endswith(".git"):
            return hgdefaultdest(source[:-4])

    if source.endswith(".git"):
        return hgdefaultdest(source[:-4])

    return hgdefaultdest(source)


hg.defaultdest = defaultdest


def getversion():
    """return version with dependencies for hg --version -v"""
    import dulwich

    dulver = ".".join(str(i) for i in dulwich.__version__)
    return __version__ + (" (dulwich %s)" % dulver)


# defend against tracebacks if we specify -r in 'hg pull'
def safebranchrevs(orig, lrepo, repo, branches, revs):
    revs, co = orig(lrepo, repo, branches, revs)
    if hgutil.safehasattr(lrepo, "changelog") and co not in lrepo.changelog:
        co = None
    return revs, co


if getattr(hg, "addbranchrevs", False):
    extensions.wrapfunction(hg, "addbranchrevs", safebranchrevs)


def extsetup(ui):
    templatekw.keywords.update({"gitnode": gitnodekw})
    revset.symbols.update({"fromgit": revset_fromgit, "gitnode": revset_gitnode})
    helpdir = os.path.join(os.path.dirname(__file__), "help")
    entry = (
        ["git"],
        _("Working with Git Repositories"),
        # Mercurial >= 3.6: doc(ui)
        lambda *args: open(os.path.join(helpdir, "git.rst")).read(),
    )
    insort(help.helptable, entry)


def reposetup(ui, repo):
    if not isinstance(repo, gitrepo.gitrepo):
        klass = hgrepo.generate_repo_subclass(repo.__class__)
        repo.__class__ = klass


if hgutil.safehasattr(manifest, "_lazymanifest"):
    # Mercurial >= 3.4
    extensions.wrapfunction(manifest.manifestdict, "diff", overlay.wrapmanifestdictdiff)


@command("gimport")
def gimport(ui, repo, remote_name=None):
    """import commits from Git to Mercurial"""
    with repo.wlock(), repo.lock():
        repo.githandler.import_commits(remote_name)


@command("gexport")
def gexport(ui, repo):
    """export commits from Mercurial to Git"""
    repo.githandler.export_commits()


@command("gclear")
def gclear(ui, repo):
    """clear out the Git cached data

    Strips all Git-related metadata from the repo, including the mapping
    between Git and Mercurial changesets. This is an irreversible
    destructive operation that may prevent further interaction with
    other clones.
    """
    repo.ui.status(_("clearing out the git cache data\n"))
    repo.githandler.clear()


@command(
    "gverify", [("r", "rev", "", _("revision to verify"), _("REV"))], _("[-r REV]")
)
def gverify(ui, repo, **opts):
    """verify that a Mercurial rev matches the corresponding Git rev

    Given a Mercurial revision that has a corresponding Git revision in the map,
    this attempts to answer whether that revision has the same contents as the
    corresponding Git revision.

    """
    ctx = scmutil.revsingle(repo, opts.get("rev"), ".")
    return verify.verify(ui, repo, ctx)


@command("git-cleanup")
def git_cleanup(ui, repo):
    """clean up Git commit map after history editing"""
    items = repo.githandler._map.items()
    if ui.configbool("hggit", "indexedlognodemap", False):
        dir = repo.sharedvfs.join(repo.githandler.map_file + "-log")
        tempdir = dir + ".temp"
        if os.path.exists(tempdir):
            hgutil.removedirs(tempdir)

        nodemap = nodemapmod.nodemap(tempdir)
        for gitsha, hgsha in items:
            if hgsha in repo:
                nodemap.add(gitsha, hgsha)
        nodemap.flush()
        with repo.wlock():
            tempdir2 = dir + ".temp2"
            hgutil.rename(dir, tempdir2)
            hgutil.rename(tempdir, dir)
            shutil.rmtree(tempdir2)

    new_map = []
    for gitsha, hgsha in items:
        if hgsha in repo:
            new_map.append("%s %s\n" % (hex(gitsha), hex(hgsha)))
    wlock = repo.wlock()
    try:
        f = repo.sharedvfs(GitHandler.map_file, "wb")
        map(f.write, new_map)
    finally:
        wlock.release()
    ui.status(_("git commit map cleaned\n"))


@command("git-updatemeta")
def gitupdatemeta(ui, repo):
    """Reads git hashes from the latest hg commits and adds them to the git-hg
    mapping."""
    with repo.wlock(), repo.lock():
        stack = repo.heads()
        githandler = repo.githandler
        parents = repo.changelog.parents
        clrev = repo.changelog.rev
        clrevision = repo.changelog.changelogrevision
        phase = repo._phasecache.phase
        public = phases.public

        seen = set(stack)
        seen.add(nullid)
        while stack:
            node = stack.pop()
            hgsha = hex(node)
            gitsha = githandler.map_git_get(hgsha)

            # If the gitsha is not already known, add it if we can
            if gitsha is None:
                gitsha = None
                commitdata = clrevision(node)
                if "convert_revision" in commitdata._rawextra:
                    gitsha = commitdata.extra.get("convert_revision")

                    # If there is no git sha, it may be a local commit. Just walk past
                    # it.
                    if gitsha:
                        githandler.map_set(gitsha, hgsha)

                if gitsha or phase(repo, clrev(node)) != public:
                    for pnode in parents(node):
                        if pnode not in seen:
                            seen.add(pnode)
                            stack.append(pnode)

        githandler.save_map(githandler.map_file)


def findcommonoutgoing(orig, repo, other, *args, **kwargs):
    if isinstance(other, gitrepo.gitrepo):
        heads = repo.githandler.get_refs(other.path)[0]
        kw = {}
        kw.update(kwargs)
        for val, k in zip(args, ("onlyheads", "force", "commoninc", "portable")):
            kw[k] = val
        force = kw.get("force", False)
        commoninc = kw.get("commoninc", None)
        if commoninc is None:
            commoninc = discovery.findcommonincoming(
                repo, other, heads=heads, force=force
            )
            kw["commoninc"] = commoninc
        return orig(repo, other, **kw)
    return orig(repo, other, *args, **kwargs)


extensions.wrapfunction(discovery, "findcommonoutgoing", findcommonoutgoing)


def getremotechanges(orig, ui, repo, other, *args, **opts):
    if isinstance(other, gitrepo.gitrepo):
        if args:
            revs = args[0]
        else:
            revs = opts.get("onlyheads", opts.get("revs"))
        r, c, cleanup = repo.githandler.getremotechanges(other, revs)
        # ugh. This is ugly even by mercurial API compatibility standards
        if "onlyheads" not in orig.func_code.co_varnames:
            cleanup = None
        return r, c, cleanup
    return orig(ui, repo, other, *args, **opts)


extensions.wrapfunction(bundlerepo, "getremotechanges", getremotechanges)


def peer(orig, uiorrepo, *args, **opts):
    newpeer = orig(uiorrepo, *args, **opts)
    if isinstance(newpeer, gitrepo.gitrepo):
        if isinstance(uiorrepo, localrepo.localrepository):
            newpeer.localrepo = uiorrepo
    return newpeer


extensions.wrapfunction(hg, "peer", peer)


def isvalidlocalpath(orig, self, path):
    return orig(self, path) or _isgitdir(path)


if hgutil.safehasattr(hgui, "path") and hgutil.safehasattr(
    hgui.path, "_isvalidlocalpath"
):
    extensions.wrapfunction(hgui.path, "_isvalidlocalpath", isvalidlocalpath)


@util.transform_notgit
def exchangepull(orig, repo, remote, heads=None, force=False, bookmarks=(), **kwargs):
    if isinstance(remote, gitrepo.gitrepo):
        # transaction manager is present in Mercurial >= 3.3
        try:
            trmanager = getattr(exchange, "transactionmanager")
        except AttributeError:
            trmanager = None
        pullop = exchange.pulloperation(repo, remote, heads, force, bookmarks=bookmarks)
        if trmanager:
            pullop.trmanager = trmanager(repo, "pull", remote.url())
        wlock = repo.wlock()
        lock = repo.lock()
        try:
            pullop.cgresult = repo.githandler.fetch(remote.path, heads)
            if trmanager:
                pullop.trmanager.close()
            else:
                pullop.closetransaction()
            return pullop
        finally:
            if trmanager:
                pullop.trmanager.release()
            else:
                pullop.releasetransaction()
            lock.release()
            wlock.release()
    else:
        return orig(repo, remote, heads, force, bookmarks=bookmarks, **kwargs)


if not hgutil.safehasattr(localrepo.localrepository, "pull"):
    # Mercurial >= 3.2
    extensions.wrapfunction(exchange, "pull", exchangepull)


@util.transform_notgit
def exchangepush(orig, repo, remote, force=False, revs=None, bookmarks=(), **kwargs):
    if isinstance(remote, gitrepo.gitrepo):
        # opargs is in Mercurial >= 3.6
        opargs = kwargs.get("opargs")
        if opargs is None:
            opargs = {}
        pushop = exchange.pushoperation(repo, remote, force, revs, bookmarks, **opargs)
        pushop.cgresult = repo.githandler.push(remote.path, revs, force)
        return pushop
    else:
        return orig(repo, remote, force, revs, bookmarks=bookmarks, **kwargs)


if not hgutil.safehasattr(localrepo.localrepository, "push"):
    # Mercurial >= 3.2
    extensions.wrapfunction(exchange, "push", exchangepush)


def revset_fromgit(repo, subset, x):
    """``fromgit()``
    Select changesets that originate from Git.
    """
    revset.getargs(x, 0, 0, "fromgit takes no arguments")
    git = repo.githandler
    node = repo.changelog.node
    return baseset(r for r in subset if git.map_git_get(hex(node(r))) is not None)


def revset_gitnode(repo, subset, x):
    """``gitnode(hash)``
    Select the changeset that originates in the given Git revision. The hash
    may be abbreviated: `gitnode(a5b)` selects the revision whose Git hash
    starts with `a5b`. Aborts if multiple changesets match the abbreviation.
    """
    args = revset.getargs(x, 1, 1, "gitnode takes one argument")
    rev = revset.getstring(args[0], "the argument to gitnode() must be a hash")
    git = repo.githandler
    node = repo.changelog.node

    def matches(r):
        gitnode = git.map_git_get(hex(node(r)))
        if gitnode is None:
            return False
        return gitnode.startswith(rev)

    result = baseset(r for r in subset if matches(r))
    if 0 <= len(result) < 2:
        return result
    raise LookupError(rev, git.map_file, _("ambiguous identifier"))


def gitnodekw(**args):
    """:gitnode: String.  The Git changeset identification hash, as a 40 char
    hexadecimal digit string."""
    node = args["ctx"]
    repo = args["repo"]
    fromextra = node.extra().get("convert_revision", "")
    if fromextra:
        return fromextra
    gitnode = repo.githandler.map_git_get(node.hex())
    if gitnode is None:
        gitnode = ""
    return gitnode
