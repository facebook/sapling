# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import errno
import hashlib
import os
import shutil
import subprocess
import textwrap
import weakref
from dataclasses import dataclass
from typing import Optional

import bindings
from edenscm import tracing

from . import bookmarks as bookmod, error, identity, progress, util
from .i18n import _
from .node import bin, hex, nullid

# If git-store is set, the path in svfs pointing to the git bare repo.
GIT_DIR_FILE = "gitdir"

# The repo is backed by a local git bare repo.
# Implies push pull should shell out to git.
GIT_STORE_REQUIREMENT = "git-store"

# Whether the repo should use git format when creating new objects.
# Should be set if git-store is set.
GIT_FORMAT_REQUIREMENT = "git"


class GitCommandError(error.Abort):
    def __init__(self, git_command, git_exitcode, git_output, **kwargs):
        self.git_command = git_command
        self.git_exitcode = git_exitcode
        self.git_output = git_output
        message = _("git command failed with exit code %d\n  %s") % (
            git_exitcode,
            git_command,
        )
        if git_output:
            message += _("\n%s") % textwrap.indent(git_output.rstrip(), "    ")
        super().__init__(message, **kwargs)


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


def isgitformat(repo):
    """Test if repo should use git format"""
    return GIT_FORMAT_REQUIREMENT in repo.storerequirements


def isgitstore(repo):
    """Test if repo is backed by a git bare repo, and should delegate to git for exchange."""
    return GIT_STORE_REQUIREMENT in repo.storerequirements


def isgitpeer(repo):
    """Test if repo should use git commands to push and pull."""
    return isgitstore(repo)


def createrepo(ui, url, destpath):
    from . import hg

    repo_config = "%include builtin:git.rc\n"
    if url:
        repo_config += "\n[paths]\ndefault = %s\n" % url

    return hg.repository(ui, destpath, create=True, initial_config=repo_config).local()


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

    destpath = ui.expandpath(destpath)

    if os.path.lexists(destpath):
        if os.path.isdir(destpath):
            if os.listdir(destpath):
                raise error.Abort(_("destination '%s' is not empty") % destpath)
        else:
            raise error.Abort(_("destination '%s' already exists") % destpath)

    try:
        repo = createrepo(ui, url, destpath)
        ret = initgitbare(ui, repo.svfs.join("git"))
        if ret != 0:
            raise error.Abort(_("git clone was not successful"))
        initgit(repo, "git", url)
        if url:
            if pullnames is None:
                pullnames = bookmod.selectivepullbookmarknames(repo)
            pull(repo, "default", names=pullnames)
    except (Exception, KeyboardInterrupt):
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

    with repo.lock():
        repo.svfs.writeutf8(GIT_DIR_FILE, gitdir)
        repo.storerequirements.add(GIT_FORMAT_REQUIREMENT)
        repo.storerequirements.add(GIT_STORE_REQUIREMENT)
        repo._writestorerequirements()
        repo.invalidatechangelog()
        visibility.add(repo, repo.changelog.dageval(lambda: heads(all())))


def maybegiturl(url):
    """Return normalized url if url is a git url, or None otherwise.

    For now url schemes "git", "git+file", "git+ftp", "git+http", "git+https",
    "git+ssh" are considered git urls. The "git+" part will be stripped.

    scp-like path "user@host:path" will be converted to "ssh://user@host/path".

    git:// and https:// urls are considered git unconditionally.
    """
    # See https://git-scm.com/docs/git-clone#_git_urls
    # user@host.xz:path/to/repo => ssh://user@host.xz/path/to/repo
    #
    # Be careful to exclude Windows file paths like "C:\foo\bar"
    if ":" in url and "//" not in url and not os.path.exists(url):
        before, after = url.split(":", 1)
        from . import hg

        if "/" not in before and before not in hg.schemes:
            url = f"git+ssh://{before}/{after}"

    parsed = util.url(url)
    if parsed.scheme in {"git", "https"}:
        return url

    # We have several test cases that rely on performing legacy (Mercurial)
    # clones for coverage.
    if parsed.scheme == "ssh" and not util.istest():
        return url

    if parsed.scheme in {
        "git+file",
        "git+ftp",
        "git+ftps",
        "git+http",
        "git+https",
        "git+ssh",
    }:
        return url[4:]

    return None


def initgitbare(ui, destpath):
    """Create a git repo into local path `dest` as a git bare repo.
    This does not prepare working copy or `.hg`, or fetch git commits.
    """
    # not using 'git clone --bare' because it writes refs to refs/heads/,
    # not in desirable refs/remotes/origin/heads/.
    cmdlist = [(None, ["init", "-q", "--bare", destpath])]
    configs = ["init.defaultBranch=_unused_branch"]
    for gitdir, cmd in cmdlist:
        ret = rungitnorepo(ui, cmd, gitdir=gitdir, configs=configs)
        if ret != 0:
            return ret
    return 0


@cached
def readgitdir(repo):
    """Return the path of the GIT_DIR, if the repo is backed by git"""
    if isgitstore(repo):
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
    config = bindings.configloader.config()
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
    """simple reference name handling for git

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
        elif all(not self.name.startswith(p) for p in ("visibleheads/", "tags/")):
            components.append("heads")
        components.append(self.name)
        return "/".join(components)

    def withremote(self, remote):
        return RefName(name=self.name, remote=remote)

    @classmethod
    def visiblehead(cls, node):
        return cls("visibleheads/%s" % hex(node))

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
        # NOTE: node will be pulled as a draft visiblehead.
        # Maybe this should be using public visibleheads once we support
        # public visibleheads.
        refspec = "+%s:%s" % (hex(node), RefName.visiblehead(node))
        refspecs.append(refspec)

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


def bundle(repo, filename, nodes):
    """create a git bundle at filename that contains nodes"""
    dag = repo.changelog.dag
    nodes = dag.sort(nodes)
    heads = dag.heads(nodes)
    bases = dag.parents(dag.roots(nodes))
    # git bundle create <file> heads... ^bases...
    args = ["bundle", "create", filename]
    # git bundle requires heads to be references.
    # find nodes that do not have bookmarks, create visiblehead refs
    anonheads = []
    for node in heads:
        bmarks = repo.nodebookmarks(node)
        if not bmarks:
            anonheads.append(node)
            args.append(str(RefName.visiblehead(node)))
        else:
            args += [str(RefName(b)) for b in bmarks]
    _writevisibleheadrefs(repo, anonheads)
    # ^ prefix excludes base nodes
    for node in bases:
        args.append("^%s" % hex(node))
    return rungit(repo, args)


def unbundle(repo, filename):
    """unpack a git bundle, return unbundled head nodes"""
    out = callgit(repo, ["bundle", "unbundle", filename])
    refmap = _parsebundleheads(out)
    # 'git bundle unbundle' does not change refs, create refs by ourselves
    _writerefs(repo, sorted(refmap.items()))
    _syncfromgit(repo)
    return list(refmap.values())


def listbundle(ui, filename):
    """return {refname: node} in a bundle"""
    out = callgitnorepo(ui, ["bundle", "list-heads", filename])
    return _parsebundleheads(out.stdout)


def isgitbundle(filename):
    """test if filename is a git bundle"""
    try:
        with open(filename, "rb") as f:
            header = f.read(16)
            # see bundle.c in git
            return header in {b"# v2 git bundle\n", b"# v3 git bundle\n"}
    except IOError as e:
        if e.errno == errno.ENOENT:
            return False
        raise


def _parsebundleheads(out):
    """return {refname: node} for 'git bundle list-heads' or 'git bundle unbundle' output"""
    refmap = {}
    for line in sorted(out.decode("utf-8").splitlines()):
        # ex. e5fc4478a3399127bac948e2c445d2e7f035a8db refs/heads/D
        hexnode, refname = line.split(" ", 1)
        node = bin(hexnode)
        refmap[refname] = node
    return refmap


def _writevisibleheadrefs(repo, nodes):
    """write visibleheads refs for nodes"""
    refnodes = [(RefName.visiblehead(n), n) for n in nodes]
    _writerefs(repo, refnodes)


def _writerefs(repo, refnodes):
    """write git references. refnodes is a list of (ref, node).

    Only 'refs/heads/<name>' references are written (as local bookmarks).
    Other references will be normalized to `refs/visibleheads/<hex>`.
    """
    for (ref, node) in refnodes:
        ref = str(ref)
        if not ref.startswith("refs/heads/"):
            # ref might be non-standard like "BUNDLE_HEAD".
            # normalize it to a visiblehead ref.
            ref = str(RefName.visiblehead(node))
        callgit(repo, ["update-ref", str(ref), hex(node)])


def _syncfromgit(repo):
    repo.invalidate(clearfilecache=True)
    repo.changelog  # trigger updating metalog


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


@cached
def _supportwritefetchhead(repo):
    """Test if 'git fetch' supports the --write-fetch-head flag"""
    # Do not use --help - it pops up a browser on Windows.
    # -h shows help in stdout and exits with code 129.
    out = callgit(repo, ["fetch", "-h"], checkreturncode=False)
    return b"--write-fetch-head" in out


def pullrefspecs(repo, url, refspecs):
    """Run `git fetch` on the backing repo to perform a pull"""
    if not refspecs:
        # Nothing to pull
        return 0
    args = ["fetch", "--no-tags", "--prune"]
    if _supportwritefetchhead(repo):
        args.append("--no-write-fetch-head")
    args += [url] + refspecs
    ret = rungit(repo, args)
    _syncfromgit(repo)
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
    # update remotenames
    if ret == 0:
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
    config = bindings.configloader.config()
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


def submodulecheckout(ctx, match=None, force=False, mctx=None):
    """Checkout commits specified in submodules

    If mctx is also provided, it is a "merge" ctx to be considered.  This
    happens during 'rebase -r mctx -d ctx'. If a submodule is only changed by
    mctx, but remains unchanged in ctx, then mctx specifies the submodule.

        o ctx (usually rebase destination, current working copy)
        |
        : o mctx (usually commit being rebased)
        | |
        | o pmctx (direct parent of mctx)
        |/
        o actx (common ancestor of ctx and mctx but is actually not considered)

    Decision table:

        ctx | pmctx | mctx | result
        --------------------------------------
        a   | a     | a    | a
        a   | a     | b    | b
        a   | b     | b    | a
        a   | b     | a    | a
        a   | b     | c    | a (with warnings)
    """
    ui = ctx.repo().ui
    if mctx:

        def adjust_submodule_node(
            node, path, mctx=mctx, pmctx=mctx.p1()
        ) -> Optional[bytes]:
            mnode = submodule_node_from_ctx_path(mctx, path)
            if mnode == node:
                return node

            pmnode = submodule_node_from_ctx_path(pmctx, path)
            if pmnode == node:
                # the "a a b => b" case in the above table
                return mnode
            elif mnode != pmnode:
                # the "a b c" case
                ui.status_err(
                    _("submodule '%s' changed by '%s' is dropped due to conflict\n")
                    % (path, mctx.shortdescription())
                )

            return node

    else:

        def adjust_submodule_node(node, path) -> Optional[bytes]:
            return node

    submodules = parsesubmodules(ctx)
    if match is not None:
        submodules = [submod for submod in submodules if match(submod.path)]
    with progress.bar(ui, _("updating"), _("submodules"), len(submodules)) as prog:
        value = 0
        for submod in submodules:
            prog.value = (value, submod.name)
            tracing.debug("checking out submodule %s\n" % submod.name)
            node = submodule_node_from_ctx_path(ctx, submod.path)
            node = adjust_submodule_node(node, submod.path)
            if node is None:
                continue
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
        ident = identity.sniffdir(repopath)
        if ident:
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
        ident = identity.sniffdir(repopath)
        if ident:
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


def callgit(repo, args, checkreturncode=True):
    """Run git command in the backing git repo, return its output"""
    gitdir = readgitdir(repo)
    ret = callgitnorepo(repo.ui, args, gitdir=gitdir)
    if checkreturncode and ret.returncode != 0:
        cmdstr = " ".join(util.shellquote(c) for c in ret.args)
        outputs = []
        if ret.stdout:
            outputs.append(ret.stdout.decode(errors="ignore"))
        if ret.stderr:
            outputs.append(ret.stderr.decode(errors="ignore"))
        output = "".join(outputs)
        raise GitCommandError(
            git_command=cmdstr,
            git_exitcode=ret.returncode,
            git_output=output,
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


def rungitnorepo(ui, args, gitdir=None, configs=None):
    """Run git command without an optional repo path, using inherited stdio.
    Passes --quiet and --verbose to the git command.
    'configs' is an optional list of configs in '<name>=<value>' format.
    """
    cmd = [gitbinary(ui)]
    if configs:
        for config in configs:
            cmd += ["-c", config]
    if gitdir is not None:
        cmd.append("--git-dir=%s" % gitdir)
    # bundle is followed by a subcommand
    if args[0] in {"bundle"}:
        gitcmd = args[0:2]
    else:
        gitcmd = args[0:1]
    cmdargs = args[len(gitcmd) :]
    cmd += gitcmd
    gitcmd = tuple(gitcmd)
    # not all git commands support --verbose or --quiet
    if ui.verbose and gitcmd in {("fetch",), ("push",)}:
        cmd.append("--verbose")
    if ui.quiet and gitcmd in {("fetch",), ("init",), ("push",), ("bundle", "create")}:
        cmd.append("--quiet")
    cmd += cmdargs
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

    def renamed(self, node):
        return False


def hashobj(kind, text):
    """(bytes, bytes) -> bytes. obtain git SHA1 hash"""
    # git blob format: kind + " " + str(size) + "\0" + text
    return hashlib.sha1(b"%s %d\0%s" % (kind, len(text), text)).digest()


def submodule_node_from_fctx(fctx) -> Optional[bytes]:
    if fctx.flags() == "m":
        fnode = fctx.filenode()
        if fnode is None:
            # workingfilectx (or overlayfilectx wrapping workingfilectx)
            # might have "None" filenode. Try to extract from "data"
            data = fctx.data()
            prefix = b"Subproject commit "
            if not data.startswith(prefix):
                raise error.ProgrammingError(f"malformed submodule data: {data}")
            fnode = bin(data[len(prefix) :].strip().decode())
        return fnode
    return None


def submodule_node_from_ctx_path(ctx, path) -> Optional[bytes]:
    """return the submodule commit hash stored in ctx's manifest tree

    If path is not a submodule or path does not exist in ctx, return None.
    """
    if path not in ctx:
        return None
    fctx = ctx[path]
    return submodule_node_from_fctx(fctx)
