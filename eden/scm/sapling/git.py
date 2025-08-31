# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import errno
import functools
import hashlib
import os
import re
import subprocess
import textwrap
import weakref
from dataclasses import dataclass
from typing import Optional, Tuple

import bindings

from sapling import tracing

from . import bookmarks as bookmod, error, identity, progress, rcutil, util
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

# Whether to be compatible with `.git/`.
DOTGIT_REQUIREMENT = "dotgit"

# ref to push to when doing commit cloud uploads
COMMIT_CLOUD_UPLOAD_REF = "refs/commitcloud/upload"


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


def createrepo(ui, url, destpath, submodule=None):
    repo_config = ""
    if url:
        repo_config += "\n[paths]\ndefault = %s\n" % url

    return setup_repository(
        ui, destpath, create=True, initial_config=repo_config, submodule=submodule
    ).local()


def setup_repository(ui, path, create=False, initial_config=None, submodule=None):
    """similar to hg.repository, but optionally sets `submodule`"""
    from . import hg

    presetupfuncs = []

    if submodule is not None:
        weak_submodule = weakref.proxy(submodule)

        def setup_submodule(ui, repo):
            repo.submodule = weak_submodule

        presetupfuncs.append(setup_submodule)

    return hg.repository(
        ui,
        path,
        create=create,
        initial_config=initial_config,
        presetupfuncs=presetupfuncs,
    )


def clone(ui, url, destpath=None, update=True, pullnames=None, submodule=None):
    """Clone a git repo, then create a repo at dest backed by the git repo.
    update can be False, or True, or a node to update to.
    - False: do not update, leave an empty working copy.
    - True: update to git HEAD.
    - other: update to `other` (node, or name).
    pullnames decides what to pull.
    - None: use default refspecs set by configs.
    - []: do not fetch anything.
    If url is empty, create the repo but do not add a remote.
    If submodule is set, it will be passed to `setup_repository`.
    """
    from . import hg

    if url.endswith("/"):
        url = url[:-1]

    if destpath is None:
        # use basename as fallback, but strip ".git" or "/.git".
        basename = os.path.basename(url)
        if basename == ".git":
            basename = os.path.basename(os.path.dirname(url))
        elif basename.endswith(".git"):
            basename = basename[:-4]
        destpath = os.path.realpath(basename)

    destpath = ui.expandpath(destpath)

    # Allow `--debug` to keep bad state for investigation.
    clean_up_on_error = not ui.debugflag

    if os.path.lexists(destpath):
        if os.path.isdir(destpath):
            clean_up_on_error = False
            if os.listdir(destpath):
                if url:
                    raise error.Abort(_("destination '%s' is not empty") % destpath)
        else:
            raise error.Abort(
                _("destination '%s' exists and is not a directory") % destpath
            )

    if clean_up_on_error:
        context = bindings.atexit.AtExit.rmtree(destpath)
    else:
        context = util.nullcontextmanager()

    with context:
        try:
            repo = createrepo(ui, url, destpath, submodule=submodule)
            ret = initgitbare(ui, repo.svfs.join("git"))
            if ret != 0:
                raise error.Abort(_("git clone was not successful"))
            repo = initgit(repo, "git")
            if url:
                if pullnames is None:
                    ls_remote_args = ["ls-remote", "--symref", url, "HEAD"]
                    symref_head_output = callgit(repo, ls_remote_args).decode("utf-8")
                    default_branch = parse_symref_head(symref_head_output)
                    if default_branch:
                        pullnames = [default_branch]
                    elif not symref_head_output:
                        # Empty string: may be empty repo?
                        pass
                    else:
                        ui.status_err(
                            _("could not parse output of '%s': %s")
                            % (
                                " ".join(ls_remote_args),
                                symref_head_output,
                            )
                        )

                if pullnames is None:
                    # If `git ls-remote --symref <url> HEAD` failed to yield a name,
                    # fall back to the using the names in the config.
                    pullnames = bookmod.selectivepullbookmarknames(repo)

                update_publicheads(repo, pullnames)

                # Make sure we pull "update". If it looks like a hash, add to
                # "nodes", otherwise to "names".
                nodes = []
                if update and update is not True:
                    if update_node := try_get_node(update):
                        nodes.append(update_node)
                    else:
                        pullnames.append(update)

                pullnames = util.dedup(pullnames)
                pull(repo, "default", names=pullnames, nodes=nodes)
        except Exception:
            repo = None
            raise

    if update is not False:
        if update is True:
            node = repo.changelog.tip()
        else:
            node = repo[update].node()
        if node is not None and node != nullid:
            hg.updatetotally(repo.ui, repo, node, None)
    return repo


def try_get_node(maybe_hex: str) -> Optional[bytes]:
    if len(maybe_hex) == 40:
        try:
            return bin(maybe_hex)
        except TypeError:
            return None


def update_publicheads(repo, pullnames):
    default_publicheads = repo.ui.configlist(
        "remotenames", "publicheads"
    )  # ['remote/master', 'remote/main']
    remote_publicheads = ["remote/" + path for path in pullnames]
    all_publicheads = ",".join(sorted(set(default_publicheads + remote_publicheads)))
    update_and_persist_config(repo, "remotenames", "publicheads", all_publicheads)


def parse_symref_head(symref_head_output: str) -> Optional[str]:
    r"""
    Args:
        symref_head_output - output of `ls-remote --symref <url> HEAD`

    >>> sapling = (
    ...     "ref: refs/heads/main\tHEAD\n"
    ...     "f58888310501872447b1b2fa4a8789210a6c6252\tHEAD\n"
    ... )
    >>> parse_symref_head(sapling)
    'main'
    >>> pytorch = (
    ...     "ref: refs/heads/master\tHEAD\n"
    ...     "8b3e35ea4aa210f48a92966e3347b78dfc6e9360\tHEAD\n"
    ... )
    >>> parse_symref_head(pytorch)
    'master'
    >>> foobar = (
    ...     "ref: refs/heads/foo/bar\tHEAD\n"
    ...     "8b3e35ea4aa210f48a92966e3347b78dfc6e9360\tHEAD\n"
    ... )
    >>> parse_symref_head(foobar)
    'foo/bar'
    >>> tag = (
    ...     "ref: refs/tags/foo\tHEAD\n"
    ...     "8b3e35ea4aa210f48a92966e3347b78dfc6e9360\tHEAD\n"
    ... )
    >>> parse_symref_head(tag) is None
    True
    >>> gerrit_refs_for = (
    ...     "ref: refs/for/master\tHEAD\n"
    ...     "8b3e35ea4aa210f48a92966e3347b78dfc6e9360\tHEAD\n"
    ... )
    >>> parse_symref_head(gerrit_refs_for) is None
    True
    >>> parse_symref_head('') is None
    True
    """
    pat = re.compile(r"^ref: ([^\t]*)\t")
    match = pat.match(symref_head_output)
    if match:
        ref = match.group(1)
        prefix = "refs/heads/"
        if ref.startswith(prefix):
            return ref[len(prefix) :]

    return None


def initgit(repo, gitdir):
    """Change a repo to be backed by a bare git repo in `gitdir`.
    This should only be called for newly created repos.
    """
    from . import visibility

    with repo.lock():
        repo.svfs.writeutf8(GIT_DIR_FILE, gitdir)
        repo.storerequirements.add(GIT_FORMAT_REQUIREMENT)
        repo.storerequirements.add(GIT_STORE_REQUIREMENT)
        repo._writestorerequirements()
    # recreate the repo to pick up key changes
    repo = setup_repository(repo.baseui, repo.root).local()
    visibility.add(repo, repo.changelog.dageval(lambda: heads(all())))
    return repo


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
    if DOTGIT_REQUIREMENT in repo.requirements:
        return repo.wvfs.join(".git")
    elif isgitstore(repo):
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
        if DOTGIT_REQUIREMENT not in repo.storerequirements:
            # The libgit2 we use has issues with the multi-pack-index.
            # Disable it for now, until we upgrade or replace libgit2.
            util.tryunlink(gitdir + "/objects/pack/multi-pack-index")
            write_maintained_git_config(repo)
        return bindings.gitstore.gitstore(gitdir, repo.ui._rcfg)


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


# By default, `git maintenance run --auto` (run by `git fetch`) triggers GC,
# which runs repack. The gc/repack can cause compatibility issues with
# shallow/not-shallow mix, such as some blob or tree cannot be read. `repack
# --filter=tree:0` might work but it can be slow.
#
# `git maintenance run --task incremental-repack` and
# `git maintenance run --task loose-objects` seem to work.
#
# `repack.writeBitmaps` is incompatible with `repack --filter...` and
# might cause issues. Therefore disable it.
#
# The multi-pack-index (incremental-repack) is incompatible with the libgit2
# we're using, unfortunately...
MAINTAINED_GIT_CONFIG = """
[maintenance "gc"]
  enabled = false
[maintenance "loose-objects"]
  enabled = true
[maintenance "incremental-repack"]
  enabled = false
[repack]
  writeBitmaps = false
"""


@cached
def write_maintained_git_config(repo):
    """Update the git repo config file so it contains the maintained config."""
    # For performance we are going to modify the git/config directly.
    gitdir = readgitdir(repo)
    config_path = os.path.join(gitdir, "config")
    try:
        old_config = util.readfileutf8(config_path)
    except FileNotFoundError:
        old_config = ""
    new_config = calculate_new_config(old_config, MAINTAINED_GIT_CONFIG)
    if new_config != old_config:
        util.replacefile(config_path, new_config.encode())


def calculate_new_config(old_config: str, maintained_config: str):
    r"""
    Examples:

    Empty config:
        >>> print(calculate_new_config('user config', ''))
        user config

    Add a maintained config section:
        >>> c = calculate_new_config('user config', 'maintained config') + 'user config2'
        >>> print(c)
        user config
        #### Begin maintained by Sapling ####
        maintained config
        #### End maintained by Sapling ####
        user config2

    No-op change to the maintained config section:
        >>> print(calculate_new_config(c, 'maintained config'))
        user config
        #### Begin maintained by Sapling ####
        maintained config
        #### End maintained by Sapling ####
        user config2

    Change the maintained config section:
        >>> print(calculate_new_config(c, 'maintained config - changed'))
        user config
        #### Begin maintained by Sapling ####
        maintained config - changed
        #### End maintained by Sapling ####
        user config2
    """
    begin_split = "\n#### Begin maintained by Sapling ####\n"
    end_split = "\n#### End maintained by Sapling ####\n"
    if begin_split in old_config and end_split in old_config:
        head, rest = old_config.split(begin_split, 1)
        old_maintained, tail = rest.split(end_split, 1)
    else:
        head = old_config
        tail = old_maintained = ""
    if old_maintained == maintained_config:
        return old_config
    return "".join([head, begin_split, maintained_config, end_split, tail])


def update_and_persist_config(repo, section, name, value):
    """edit config and save it to the repo's config file"""
    configfilename = repo.ui.identity.configrepofile()
    configfilepath = repo.localvfs.join(configfilename)
    rcutil.editconfig(repo.ui, configfilepath, section, name, value)


@dataclass
class RefName:
    """simple reference name handling for git

    Common reference names examples:
    - refs/heads/foo           # branch "foo"
    - refs/tags/v1.0           # tag "v1.0"
    - refs/remotes/origin/foo  # branch "foo" in "origin" (note: no "heads/")

    Note that tags are special. Git writes remote tags to "refs/tags/<tagname>"
    and do not keep tags under "refs/remotes". But the tags are more like remote
    names (immutable, different remotes might have different tags). So we put
    tags in "refs/remotetags/<remote>/<tagname>" and sync them in metalog as
    "<remote>/tags/<tagname>".
    """

    name: str
    remote: str = ""

    def __str__(self):
        components = ["refs"]
        name = self.name
        if self.remote:
            if self.name.startswith("tags/"):
                components += ["remotetags", self.remote]
                name = name[len("tags/") :]
            else:
                components += ["remotes", self.remote]
        elif self.name.startswith("refs/"):
            # This allows pushing to arbitrary git server-side refs which is useful with
            # some servers like Gerrit which uses refs/for/master for sending changes for
            # code review or mononoke's refs/commitcloud/upload.
            return self.name
        elif all(
            not self.name.startswith(p)
            for p in ("visibleheads/", "remotetags/", "tags/")
        ):
            components.append("heads")
        components.append(name)
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

    names will be normalized to remote heads or tags, if starts with 'tags/'.
    missing names will be removed.
    nodes, if pulled, will be written to "visibleheads".
    """
    url, remote = urlremote(repo.ui, source)

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
    for ref, node in refnodes:
        ref = str(ref)
        if not ref.startswith("refs/heads/"):
            # ref might be non-standard like "BUNDLE_HEAD".
            # normalize it to a visiblehead ref.
            ref = str(RefName.visiblehead(node))
        callgit(repo, ["update-ref", str(ref), hex(node)])


def _syncfromgit(repo, refnames=None):
    """If refnames is set, sync just the given references.
    Otherwise, invalidate everything and reload changelog+metalog.
    """
    if refnames is not None:
        metalog = repo.metalog()
        repo.changelog.inner.import_external_references(metalog, refnames)
    else:
        repo.invalidate(clearfilecache=True)
        repo.changelog  # trigger updating metalog


def urlremote(ui, source):
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
    if repo.ui.configbool("git", "shallow"):
        filter_config = repo.ui.config("git", "filter")
        if filter_config:
            args.append(f"--filter={filter_config}")
    if _supportwritefetchhead(repo):
        args.append("--no-write-fetch-head")
    args += [url] + refspecs
    with repo.lock():
        ret = rungit(repo, args)
        if ret == 0:
            refnames = [s.split(":", 1)[1] for s in refspecs if ":" in s]
            with repo.transaction("pull"):
                _syncfromgit(repo, refnames)
    return ret


def push(repo, dest, pushnode_to_pairs, force=False):
    """Push "pushnode" to remote "dest" bookmark "to"

    `pushnode_to_pairs` is a list of `(pushnode, to)` pairs.

    If force is True, enable non-fast-forward moves.
    If pushnode is None, delete the remote bookmark.
    """
    url, remote = urlremote(repo.ui, dest)
    refspecs = []
    for pushnode, to in pushnode_to_pairs:
        if pushnode is None:
            fromspec = ""
        elif force:
            fromspec = "+%s" % hex(pushnode)
        else:
            fromspec = "%s" % hex(pushnode)
        refname = RefName(name=to)
        refspec = "%s:%s" % (fromspec, refname)
        refspecs.append(refspec)
    if not refspecs:
        return 0
    with repo.lock(), repo.transaction("push"):
        ret = rungit(repo, ["push", url, *refspecs])
        # update remotenames
        if ret == 0:
            name = refname.withremote(remote).remotename
            metalog = repo.metalog()
            namenodes = bookmod.decoderemotenames(metalog["remotenames"])
            if pushnode is None:
                namenodes.pop(name, None)
            else:
                if not to.startswith(COMMIT_CLOUD_UPLOAD_REF):
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
    out = callgit(repo, ["ls-remote", "--refs", url, *patterns])
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
    submodules = []
    try:
        origin_url = repo.ui.paths["default"].loc or None
    except KeyError:
        origin_url = None
    for s in bindings.submodule.parse_gitmodules(data, origin_url, repo.ui._rcfg):
        submodules.append(
            Submodule(s["name"], s["url"], s["path"], weakref.proxy(repo)),
        )

    return submodules


def maybe_cleanup_submodule_in_treestate(repo):
    """Remove treestate submodule entries, based on '.gitmodules' in the working copy.

    By the current design, treestate should not track submodules. However,
    those entries sometimes (mistakenly?) got in the treestate. This function
    cleans them up.
    """
    if (
        "treestate" not in repo.requirements
        or GIT_FORMAT_REQUIREMENT not in repo.storerequirements
    ):
        # No need to cleanup if treestate is not used (ex. dotgit), or if Git format is not used.
        return

    data = repo.wvfs.tryread(".gitmodules")
    if not data:
        return

    remove = repo.dirstate._map._tree.remove
    for s in bindings.submodule.parse_gitmodules(data):
        remove(s["path"])


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

        The repo will be created at:
        <parent repo>/.hg/store/gitmodules/<escaped submodule name>
        """
        urldigest = hashlib.sha1(self.url.encode("utf-8")).hexdigest()
        repopath = self.gitmodulesvfs.join("gitmodules", urldigest)
        ident = identity.sniffdir(repopath)
        if ident:
            repo = setup_repository(self.parentrepo.baseui, repopath, submodule=self)
        else:
            # create the repo but do not fetch anything
            repo = clone(
                self.parentrepo.baseui,
                self.url,
                destpath=repopath,
                update=False,
                pullnames=[],
                submodule=self,
            )
        self._inherit_git_config(repo)
        return repo

    @util.propertycache
    def workingcopyrepo(self):
        """submodule working repo created on demand

        The repo will be created in the parent repo's working copy, and share
        the backing repo.
        """
        if "eden" in self.parentrepo.requirements:
            # NOTE: maybe edenfs redirect can be used here?
            # or, teach edenfs about the nested repos somehow?
            raise error.Abort(_("submodule checkout in edenfs is not yet supported"))
        from . import hg

        ui = self.parentrepo.ui
        repopath = self.parentrepo.wvfs.join(self.path)
        ident = identity.sniffdir(repopath)
        if ident:
            ui.debug(" initializing submodule workingcopy at %s\n" % repopath)
            repo = setup_repository(self.parentrepo.baseui, repopath, submodule=self)
        else:
            if self.parentrepo.wvfs.isfile(self.path):
                ui.debug(" unlinking conflicted submodule file at %s\n" % self.path)
                self.parentrepo.wvfs.unlink(self.path)
            self.parentrepo.wvfs.makedirs(self.path)
            backingrepo = self.backingrepo
            ui.debug(
                " creating submodule workingcopy at %s with backing repo %s\n"
                % (repopath, backingrepo.root)
            )
            # Prefer parentrepo's dotdir.
            share_ui = backingrepo.ui
            if share_ui.identity.dotdir() != ui.identity.dotdir():
                share_ui = share_ui.copy()
                share_ui.identity = ui.identity
            repo = hg.share(
                share_ui,
                backingrepo.root,
                repopath,
                update=False,
                relative=True,
                repository=functools.partial(setup_repository, submodule=self),
            )
        self._inherit_git_config(repo)
        return repo

    def _inherit_git_config(self, subrepo):
        """Inherit [git] configs from parentrepo to subrepo"""
        for name, value in self.parentrepo.ui.configitems("git"):
            if subrepo.ui.config("git", name) is None:
                subrepo.ui.setconfig("git", name, value, "parent")

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
            self._pullraw(repo, hex(node))

    def pullhead(self, repo):
        self._pullraw(repo, "HEAD")

    def _pullraw(self, repo, refspec_lhs):
        repo.ui.status(_("pulling submodule %s\n") % self.nestedpath)
        # Write a remote bookmark to mark node public
        quiet = repo.ui.configbool("experimental", "submodule-pull-quiet", True)
        with repo.ui.configoverride({("ui", "quiet"): str(quiet)}):
            refspec = "+%s:refs/remotes/parent/%s" % (
                refspec_lhs,
                # Avoids conflicts like gflags/ and gflags/doc sharing a
                # same backing repo. (Git does not allow one reference
                # "gflags" to be a prefix of another reference
                # "gflags/doc").
                self.nestedpath.replace("_", "__").replace("/", "_"),
            )
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

        if node not in repo:
            # `node` does not exist after pull. Try to pull "HEAD" as a mitigation.
            # NOTE: See `man gitmodules`. If "branch" is specified, then this
            # should probably pull the specified branch instead.
            self.pullhead(repo)
            # Track whether pullhead fixed the issue.
            fixed = node in repo
            repo.ui.log(
                "features",
                feature="submodule-pullhead",
                message=f"fixed: {fixed} node: {hex(node)} submod: {repr(self)}",
            )

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
        dotgit_path = os.path.join(repopath, ".git")

        if DOTGIT_REQUIREMENT in self.parentrepo.requirements and os.path.exists(
            dotgit_path
        ):
            # dotgit repo, .git/sl not yet initialized.
            # read git HEAD directly.
            git = bindings.gitcompat.BareGit(dotgit_path, self.parentrepo.ui._rcfg)

            return git.resolve_head()
        else:
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
    # use ui.system, which is compatible with chg, but goes through shell
    return ui.system(cmd)


def gitbinary(ui):
    """return git executable"""
    return ui.config("ui", "git") or "git"


class gitfilelog:
    """filelog-like interface for git"""

    def __init__(self, repo):
        self.store = repo.fileslog.filestore

    def lookup(self, node):
        assert len(node) == 20
        return node

    def read(self, node):
        return self.store.readobj(node, "blob")

    def revision(self, node, raw=False):
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


def author_date_from_extras(extra) -> Optional[Tuple[int, int]]:
    """Extract the author date from commit extras.

    This is encoded using the 'author_date' extra.
    """
    d = extra.get("author_date")
    if d:
        try:
            sec_str, tz_str = d.split(" ", 1)
            return int(sec_str), int(tz_str)
        except ValueError:
            pass
    return None


def committer_and_date_from_extras(extra) -> Optional[Tuple[str, int, int]]:
    """Extract the committer and committer date from commit extras.

    There are two ways that this may have been encoded:

      * Separate 'committer' and 'committer_date' extras.  This is used by
        Sapling.
      * A single 'committer' extra, where the two fields are stored separated
        by a space.  This is used by hg-git and Mononoke.
    """
    if "committer" in extra and "committer_date" in extra:
        try:
            sec_str, tz_str = extra["committer_date"].split(" ", 1)
            return extra["committer"], int(sec_str), int(tz_str)
        except ValueError:
            pass
    elif "committer" in extra:
        try:
            committer, sec_str, tz_str = extra["committer"].rsplit(" ", 2)
            return committer, int(sec_str), int(tz_str)
        except ValueError:
            pass
    return None


def update_extra_with_git_committer(ui, ctx, extra):
    """process Git committer on local commit creation

    Update the `extra` in place to contain the Git committer and committer date information.
    """
    committer = ui.config("git", "committer") or ui.username()
    extra["committer"] = committer

    date = ui.config("git", "committer-date") or "now"
    unixtime, offset = util.parsedate(date)
    committer_date = f"{unixtime} {offset}"
    extra["committer_date"] = committer_date
