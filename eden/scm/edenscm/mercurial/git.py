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

import bindings
from edenscm import tracing

from . import error, util
from .i18n import _
from .node import hex

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


def clone(ui, url, destpath=None, update=True):
    """Clone a git repo, then create a repo at dest backed by the git repo.
    update can be False, or True, or a node to update to.
    - False: do not update, leave an empty working copy.
    - True: upate to git HEAD.
    - other: update to `other` (node, or name).
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
        ret = clonegitbare(ui, url, repo.svfs.join("git"))
        if ret != 0:
            raise error.Abort(_("git clone was not successful"))
        initgit(repo, "git")
    except Exception:
        repo = None
        shutil.rmtree(destpath, ignore_errors=True)
        raise
    if update is not False:
        if update is True:
            update = None
        postpullupdate(repo, update)
    return repo


def initgit(repo, gitdir):
    """Change a repo to be backed by a bare git repo in `gitdir`.
    This should only be called for newly created repos.
    """
    from . import visibility

    hgrc = "%include builtin:git.rc\n"

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


def clonegitbare(ui, giturl, destpath):
    """Clone a git repo into local path `dest` as a git bare repo.
    This does not prepare working copy or `.hg`.
    If giturl is empty, do not add a remote and skip fetching.
    """
    # not using 'git clone --bare' because it writes refs to refs/heads/,
    # not in desirable refs/remotes/origin/heads/.
    cmdlist = [(None, ["init", "-q", "-b", "default", "--bare", destpath])]
    if giturl:
        cmdlist += [
            (destpath, ["remote", "add", "origin", giturl]),
            (destpath, ["fetch", "--no-tags", "origin"]),
        ]
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


def revparse(repo, revspec):
    parsed = callgit(repo, ["rev-parse", revspec])
    return parsed.decode("utf-8", "surrogateescape").strip()


def pull(repo, source, refspecs):
    """Run `git fetch` on the backing repo to perform a pull"""
    ret = rungit(repo, ["fetch", "--no-tags", "--prune", source] + refspecs)
    repo.invalidate(clearfilecache=True)
    return ret


def postpullupdate(repo, node=None):
    """Checkout the pulled commit. If `node` is None, use FETCH_HEAD"""
    from . import hg

    if node is None:
        # If the repo is empty, then FETCH_HEAD cannot be resolved.
        if len(repo) == 0:
            return
        fetchhead = revparse(repo, "FETCH_HEAD")
        node = repo[fetchhead].node()
    return hg.updatetotally(repo.ui, repo, node, None)


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
    refspec = "%s:refs/heads/%s" % (fromspec, to)
    ret = rungit(repo, ["push", "-u", dest, refspec])
    repo.invalidatechangelog()
    return ret


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
