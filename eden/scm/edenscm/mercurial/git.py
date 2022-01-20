# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import hashlib
import os
import shutil
import subprocess
import weakref
from dataclasses import dataclass

import bindings
from edenscm import tracing

from . import bookmarks as bookmod, error, progress, util
from .i18n import _
from .node import bin, hex, nullid

GIT_DIR_FILE = "gitdir"
GIT_REQUIREMENT = "git"


def cached(func):
    def wrapper(repo, *args, **kwargs):
        key = "_git_%s" % func.__name__
        cached = repo.__dict__.get(key, None)
        if cached is None:
            value = func(repo, *args, **kwargs)
            repo.__dict__[key] = (value,)
            return value
        else:
            return cached[0]

    return wrapper


def isgit(repo):
    """Test if repo is backed by git"""
    return GIT_REQUIREMENT in repo.storerequirements


def clone(ui, url, destpath=None, update=True, pullnames=None):
    """Clone a git repo, then create a repo at dest backed by the git repo.
    update can be False, or True, or a node to update to.
    - False: do not update, leave an empty working copy.
    - True: upate to git HEAD.
    - other: update to `other` (node, or name).
    pullnames decides what to pull.
    - None: use default refspecs set by configs.
    - []: do not fetch anything.
    If url is empty, create the repo but do not add a remote.
    """
    from . import hg

    if destpath is None:
        # use basename as fallback, but strip ".git" or "/.git".
        basename = os.path.basename(url)
        if basename == ".git":
            basename = os.path.basename(os.path.dirname(url))
        elif basename.endswith(".git"):
            basename = basename[:-4]
        destpath = os.path.realpath(basename)

    repo = hg.repository(ui, ui.expandpath(destpath), create=True).local()
    try:
        ret = initgitbare(ui, url, repo.svfs.join("git"))
        if ret != 0:
            raise error.Abort(_("git clone was not successful"))
        initgit(repo, "git", url)
        if url:
            if pullnames is None:
                pullnames = bookmod.selectivepullbookmarknames(repo)
            pull(repo, "default", names=pullnames)
    except Exception:
        repo = None
        shutil.rmtree(destpath, ignore_errors=True)
        raise
    if update is not False:
        if update is True:
            node = repo.changelog.tip()
        else:
            node = repo[update].node()
        if node is not None and node != nullid:
            hg.updatetotally(repo.ui, repo, node, None)
    return repo


def initgit(repo, gitdir, giturl=None):
    """Change a repo to be backed by a bare git repo in `gitdir`.
    This should only be called for newly created repos.
    """
    from . import visibility

    hgrc = "%include builtin:git.rc\n"
    if giturl:
        hgrc += "\n[paths]\ndefault = %s\n" % giturl

    with repo.lock(), repo.transaction("initgit"):
        repo.svfs.writeutf8(GIT_DIR_FILE, gitdir)
        repo.storerequirements.add(GIT_REQUIREMENT)
        repo._writestorerequirements()
        repo.invalidatechangelog()
        visibility.add(repo, repo.changelog.dageval(lambda: heads(all())))
        repo.sharedvfs.writeutf8("hgrc", hgrc)
        repo.ui.reloadconfigs(repo.root)


def maybegiturl(url):
    """Return normalized url if url is a git url, or None otherwise.

    For now url schemes "git", "git+file", "git+ftp", "git+http", "git+https",
    "git+ssh" are considered git urls. The "git+" part will be stripped.
    """
    parsed = util.url(url)
    if parsed.scheme == "git":
        return url
    if parsed.scheme in {
        "git+file",
        "git+ftp",
        "git+ftps",
        "git+http",
        "git+https",
        "git+ssh",
    }:
        if url.startswith("git+"):
            return url[4:]
    return None


def initgitbare(ui, giturl, destpath):
    """Create a git repo into local path `dest` as a git bare repo.
    This does not prepare working copy or `.hg`, or fetch git commits.
    If giturl is empty, do not add a remote.
    """
    # not using 'git clone --bare' because it writes refs to refs/heads/,
    # not in desirable refs/remotes/origin/heads/.
    cmdlist = [(None, ["init", "-q", "-b", "default", "--bare", destpath])]
    for gitdir, cmd in cmdlist:
        ret = rungitnorepo(ui, cmd, gitdir=gitdir)
        if ret != 0:
            return ret
    return 0


@cached
def readgitdir(repo):
    """Return the path of the GIT_DIR, if the repo is backed by git"""
    if isgit(repo):
        path = repo.svfs.readutf8(GIT_DIR_FILE)
        if os.path.isabs(path):
            return path
        else:
            return repo.svfs.join(path)
    else:
        return None


def openstore(repo):
    """Obtain a gitstore object to access git odb"""
    gitdir = readgitdir(repo)
    if gitdir:
        return bindings.gitstore.gitstore(gitdir)


@cached
def readconfig(repo):
    """Read git config into a config object"""
    out = callgit(repo, ["config", "-l"])
    config = bindings.configparser.config()
    for line in out.splitlines():
        line = line.decode("utf-8", "surrogateescape")
        if "=" not in line:
            continue
        sectionname, value = line.split("=", 1)
        if "." not in sectionname:
            continue
        section, name = sectionname.split(".", 1)
        config.set(section, name, value, "git")
    return config


@dataclass
class RefName:
    """simple reference name handing for git

    Common reference names examples:
    - refs/heads/foo           # branch "foo"
    - refs/tags/v1.0           # tag "v1.0"
    - refs/remotes/origin/foo  # branch "foo" in "origin" (note: no "heads/")

    Note that tags are special. Git writes remote tags to "refs/tags/" and do
    not keep tags under "refs/remotes". But here we put tags in "refs/remotes"
    so they can be used like other remote names.
    """

    name: str
    remote: str = ""

    def __str__(self):
        components = ["refs"]
        if self.remote:
            components += ["remotes", self.remote]
        elif not self.name.startswith("tags/"):
            components.append("heads")
        components.append(self.name)
        return "/".join(components)

    def withremote(self, remote):
        return RefName(name=self.name, remote=remote)

    @classmethod
    def visiblehead(cls, node, remote=""):
        return cls("visibleheads/%s" % hex(node), remote=remote)

    @property
    def remotename(self):
        """remotename used in the local (hg) repo"""
        return "%s/%s" % (self.remote or "origin", self.name)


def revparse(repo, revspec):
    parsed = callgit(repo, ["rev-parse", revspec])
    return parsed.decode("utf-8", "surrogateescape").strip()


def pull(repo, source, names=(), nodes=()):
    """Pull specified revisions and names.

    names will be normalized to remote heads or tags, if starts wtih 'tags/'.
    missing names will be removed.
    nodes, if pulled, will be written to "visibleheads".
    """
    url, remote = _urlremote(repo.ui, source)

    # normalize names for listing
    refnames = [RefName(name) for name in names]
    listed = listremote(repo, url, refnames)  # ex. {'refs/heads/main': node}

    refspecs = []
    for refname in refnames:
        node = listed.get(str(refname))
        existingnode = repo._remotenames.get(refname.remotename)
        if node == existingnode:
            # not changed
            continue
        if node is None:
            # TODO: Figure out how to remove refs.
            # refspec ":refs/..." does not seem to work reliably.
            continue
        else:
            # pull the node explicitly
            refspec = "+%s:%s" % (hex(node), refname.withremote(remote))
        refspecs.append(refspec)

    for node in nodes:
        refspec = "+%s:%s" % (hex(node), RefName.visiblehead(node, remote=remote))

    ret = pullrefspecs(repo, url, refspecs)

    # update "tip", useful for pull --checkout
    tip = None
    for refname in refnames:
        node = repo._remotenames.get(refname.remotename)
        if node is not None:
            tip = node
    if tip is None:
        tip = repo.changelog.dag.all().first()
    if tip is not None:
        with repo.lock(), repo.transaction("pull"):
            metalog = repo.metalog()
            metalog["tip"] = tip
            metalog.commit("hg pull\nTransaction: pull")

    return ret


def _urlremote(ui, source):
    """normalize source into (url, remotename)"""
    source = source or "default"
    if source in ui.paths:
        url = ui.paths[source].rawloc
    else:
        url = source
        name = ui.paths.getname(source)
        if not name:
            hint = _("use '@prog@ paths -a NAME %s' to add a remote name") % url
            raise error.Abort(_("remote url %s does not have a name") % url, hint=hint)
        source = name

    # respect remotenames.rename.<source> config
    remote = ui.config("remotenames", "rename.%s" % source) or source

    return (url, remote)


def pullrefspecs(repo, url, refspecs):
    """Run `git fetch` on the backing repo to perform a pull"""
    if not refspecs:
        # Nothing to pull
        return 0
    ret = rungit(
        repo,
        ["fetch", "--no-write-fetch-head", "--no-tags", "--prune", url] + refspecs,
    )
    repo.invalidate(clearfilecache=True)
    repo.changelog  # trigger updating metalog
    return ret


def push(repo, dest, pushnode, to, force=False):
    """Push "pushnode" to remote "dest" bookmark "to"

    If force is True, enable non-fast-forward moves.
    If pushnode is None, delete the remote bookmark.
    """
    if pushnode is None:
        fromspec = ""
    elif force:
        fromspec = "+%s" % hex(pushnode)
    else:
        fromspec = "%s" % hex(pushnode)

    url, remote = _urlremote(repo.ui, dest)
    refname = RefName(name=to)
    refspec = "%s:%s" % (fromspec, refname)
    ret = rungit(repo, ["push", url, refspec])
    # udpate remotenames
    name = refname.withremote(remote).remotename
    with repo.lock(), repo.transaction("push"):
        metalog = repo.metalog()
        namenodes = bookmod.decoderemotenames(metalog["remotenames"])
        if pushnode is None:
            namenodes.pop(name, None)
        else:
            namenodes[name] = pushnode
        metalog["remotenames"] = bookmod.encoderemotenames(namenodes)
    return ret


def listremote(repo, url, patterns):
    """List references of the remote peer
    Return a dict of name to node.
    """
    patterns = [str(p) for p in patterns]
    if not patterns:
        return {}
    out = callgit(repo, ["ls-remote", "--refs", url] + patterns)
    refs = {}
    for line in out.splitlines():
        if b"\t" not in line:
            continue
        hexnode, name = line.split(b"\t", 1)
        refs[name.decode("utf-8")] = bin(hexnode)
    return refs


@cached
def parsesubmodules(ctx):
    """Parse .gitmodules in ctx. Return [Submodule]."""
    repo = ctx.repo()
    if not repo.ui.configbool("git", "submodules"):
        repo.ui.note(_("submodules are disabled via git.submodules\n"))
        return {}
    if ".gitmodules" not in ctx:
        return {}

    data = ctx[".gitmodules"].data()
    # strip leading spaces
    data = b"".join(l.strip() + b"\n" for l in data.splitlines())
    config = bindings.configparser.config()
    config.parse(data.decode("utf-8", "surrogateescape"), ".gitmodules")
    prefix = 'submodule "'
    suffix = '"'
    submodules = []
    for section in config.sections():
        if section.startswith(prefix) and section.endswith(suffix):
            subname = section[len(prefix) : -len(suffix)]
        subconfig = {}
        for name in config.names(section):
            value = config.get(section, name)
            subconfig[name] = value
        if "url" in subconfig and "path" in subconfig:
            submodules.append(
                Submodule(
                    subname.replace(".", "_"),
                    subconfig["url"],
                    subconfig["path"],
                    weakref.proxy(repo),
                )
            )
    return submodules


def submodulecheckout(ctx, match=None, force=False):
    """Checkout commits specified in submodules"""
    ui = ctx.repo().ui
    submodules = parsesubmodules(ctx)
    if match is not None:
        submodules = [submod for submod in submodules if match(submod.path)]
    with progress.bar(ui, _("updating"), _("submodules"), len(submodules)) as prog:
        value = 0
        for submod in submodules:
            prog.value = (value, submod.name)
            tracing.debug("checking out submodule %s\n" % submod.name)
            if submod.path not in ctx:
                continue
            fctx = ctx[submod.path]
            if fctx.flags() != "m":
                continue
            node = fctx.filenode()
            submod.checkout(node, force=force)
            value += 1


@cached
def submodulestatus(ctx):
    """Find submodule working parents changes.
    Return submodules {path: (oldnode, newnode)}.
    Both oldnode and newnode are nullable.
    """
    assert ctx.node() is None, "ctx should be a workingctx"
    tree = ctx.p1().manifest()
    submodules = parsesubmodules(ctx)
    status = {}
    for submod in submodules:
        oldnode = tree.get(submod.path)
        newnode = submod.workingparentnode()
        if newnode == nullid:
            newnode = None
        if newnode is None and oldnode is None:
            # Treat it as not a submodule.
            continue
        status[submod.path] = (oldnode, newnode)
    return status


@dataclass
class Submodule:
    name: str
    url: str
    path: str
    parentrepo: object

    @util.propertycache
    def backingrepo(self):
        """submodule backing repo created on demand

        The repo will be crated at:
        <parent repo>/.hg/store/gitmodules/<escaped submodule name>
        """
        urldigest = hashlib.sha1(self.url.encode("utf-8")).hexdigest()
        repopath = self.gitmodulesvfs.join("gitmodules", urldigest)
        if os.path.isdir(os.path.join(repopath, ".hg")):
            from . import hg

            repo = hg.repository(self.parentrepo.baseui, repopath)

        else:
            # create the repo but do not fetch anything
            repo = clone(
                self.parentrepo.baseui,
                self.url,
                destpath=repopath,
                update=False,
                pullnames=[],
            )
        repo.submodule = weakref.proxy(self)
        return repo

    @util.propertycache
    def workingcopyrepo(self):
        """submodule working repo created on demand

        The repo will be crated in the parent repo's working copy, and share
        the backing repo.
        """
        if "eden" in self.parentrepo.requirements:
            # NOTE: maybe edenfs redirect can be used here?
            # or, teach edenfs about the nested repos somehow?
            raise error.Abort(_("submodule checkout in edenfs is not yet supported"))
        from . import hg

        repopath = self.parentrepo.wvfs.join(self.path)
        if os.path.isdir(os.path.join(repopath, ".hg")):

            repo = hg.repository(self.parentrepo.baseui, repopath)
        else:
            if self.parentrepo.wvfs.isfile(self.path):
                self.parentrepo.wvfs.unlink(self.path)
            self.parentrepo.wvfs.makedirs(self.path)
            backingrepo = self.backingrepo
            repo = hg.share(
                backingrepo.ui, backingrepo.root, repopath, update=False, relative=True
            )
        repo.submodule = weakref.proxy(self)
        return repo

    @util.propertycache
    def gitmodulesvfs(self):
        """Follow a chain of nested parents, get the svfs"""
        repo = self.parentrepo
        while True:
            submod = getattr(repo, "submodule", None)
            if submod is None:
                break
            repo = submod.parentrepo
        return weakref.proxy(repo.svfs)

    @util.propertycache
    def nestedpath(self):
        """Follow a chain of nested parents, get the full path of subrepo.
        For display purpose only.
        """
        path = self.path
        repo = self.parentrepo
        while True:
            submod = getattr(repo, "submodule", None)
            if submod is None:
                break
            path = "%s/%s" % (submod.path, path)
            repo = submod.parentrepo
        return path

    def pullnode(self, repo, node):
        """fetch a commit on demand, prepare for checkout"""
        if node not in repo:
            repo.ui.status(_("pulling submodule %s\n") % self.nestedpath)
            # Write a remote bookmark to mark node public
            with repo.ui.configoverride({("ui", "quiet"): "true"}):
                refspec = "+%s:refs/remotes/parent/%s" % (hex(node), self.nestedpath)
                pullrefspecs(repo, self.url, [refspec])

    def checkout(self, node, force=False):
        """checkout a commit in working copy"""
        # Try to check working parent without constructing the repo.
        # This can speed up checkout significantly if there are many
        # submodules.
        if not force and self.workingparentnode() == node:
            return

        repo = self.workingcopyrepo
        self.pullnode(repo, node)
        # Skip if the commit is already checked out, unless force is set.
        if not force and repo["."].node() == node:
            return
        # Run checkout
        from . import hg

        hg.updaterepo(repo, node, overwrite=force)

    def workingparentnode(self):
        """get the working parent node (in a fast way)"""
        # try propertycache workingcopyrepo first
        repo = self.__dict__.get("workingcopyrepo", None)
        if repo is not None:
            return repo.dirstate.p1()

        repopath = self.parentrepo.wvfs.join(self.path)

        from . import dirstate

        return dirstate.fastreadp1(repopath)


def callgit(repo, args):
    """Run git command in the backing git repo, return its output"""
    gitdir = readgitdir(repo)
    ret = callgitnorepo(repo.ui, args, gitdir=gitdir)
    if ret.returncode != 0:
        cmdstr = " ".join(util.shellquote(c) for c in ret.args)
        raise error.Abort(
            _("git command (%s) failed with exit code %s:\n%s%s")
            % (cmdstr, ret.returncode, ret.stdout, ret.stderr)
        )
    return ret.stdout


def callgitnorepo(ui, args, gitdir=None):
    """Run git command, return its `CompletedProcess`"""
    cmd = [gitbinary(ui)]
    if gitdir is not None:
        cmd.append("--git-dir=%s" % gitdir)
    cmd += args
    return subprocess.run(cmd, capture_output=True)


def rungit(repo, args):
    """Run git command in the backing git repo, using inherited stdio.
    Passes --quiet and --verbose to the git command.
    """
    gitdir = readgitdir(repo)
    return rungitnorepo(repo.ui, args, gitdir=gitdir)


def rungitnorepo(ui, args, gitdir=None):
    """Run git command without an optional repo path, using inherited stdio.
    Passes --quiet and --verbose to the git command.
    """
    cmd = [gitbinary(ui)]
    if gitdir is not None:
        cmd.append("--git-dir=%s" % gitdir)
    gitcmd = args[0]
    cmd.append(gitcmd)
    # not all git commands support --verbose or --quiet
    if ui.verbose and gitcmd in {"fetch", "push"}:
        cmd.append("--verbose")
    if ui.quiet and gitcmd in {"fetch", "init", "push"}:
        cmd.append("--quiet")
    cmd += args[1:]
    cmd = " ".join(util.shellquote(c) for c in cmd)
    tracing.debug("running %s\n" % cmd)
    # use ui.system, which is compatibile with chg, but goes through shell
    return ui.system(cmd)


def gitbinary(ui):
    """return git executable"""
    return ui.config("ui", "git") or "git"


class gitfilelog(object):
    """filelog-like interface for git"""

    def __init__(self, repo):
        self.store = repo.fileslog.contentstore

    def lookup(self, node):
        assert len(node) == 20
        return node

    def read(self, node):
        return self.store.readobj(node, "blob")

    def size(self, node):
        return self.store.readobjsize(node, "blob")

    def rev(self, node):
        # same trick as remotefilelog
        return node

    def cmp(self, node, text):
        """returns True if blob hash is different from text"""
        # compare without reading `node`
        return node != hashobj(b"blob", text)


def hashobj(kind, text):
    """(bytes, bytes) -> bytes. obtain git SHA1 hash"""
    # git blob format: kind + " " + str(size) + "\0" + text
    return hashlib.sha1(b"%s %d\0%s" % (kind, len(text), text)).digest()
