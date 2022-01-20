# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import hashlib
import subprocess

import bindings

from . import error, util
from .i18n import _

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


@cached
def readgitdir(repo):
    """Return the path of the GIT_DIR, if the repo is backed by git"""
    if isgit(repo):
        return repo.svfs.readutf8(GIT_DIR_FILE)
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
