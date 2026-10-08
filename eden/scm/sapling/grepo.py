# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

"""
utilities to support the git repo tool

References:
    https://gerrit.googlesource.com/git-repo
"""

import weakref
from typing import List

from . import git
from .git import Submodule

# Whether to be compatible with `.repo/`.
GREPO_REQUIREMENT = "grepo"


def checkout_manifest(repo, source, target, force, dry_run=False) -> None:
    """Update the manifests Git index and worktree from `source` to `target`.

    This does not move the manifests HEAD. With `force`, Git discards local
    changes. Without `force`, Git refuses local changes to files that differ
    between `source` and `target`. With `dry_run`, Git writes nothing. A refusal
    raises the same error as a failed project checkout.
    """
    manifest_root = repo.wvfs.join(".repo/manifests")
    args = ["--work-tree=%s" % manifest_root, "read-tree", "-u"]
    if dry_run:
        args.append("-n")
    if force:
        args.extend(["--reset", target.hex()])
    else:
        args.extend(["-m", source.hex(), target.hex()])
    try:
        git.callgit(repo, args)
    except git.GitCommandError as ex:
        raise git._projectcheckoutabort([(".repo/manifests", target.node(), ex)])


def getgreposubmodules(ctx, repo) -> List[Submodule]:
    """Synthesize Submodule objects from the parent manifest for Grepo repos.

    Conceptually, grepo projects are very similar to Git submodules. Rust tree
    resolver synthesizes them into tree manifest as GitSubmodule (flag "m" in Python).
    This method synthesizes them into Python Submodule objects to plug into
    existing infrastructure.
    """
    parent = ctx.p1() if ctx.node() is None else ctx
    mf = parent.manifest()
    submodules = []
    for path in mf:
        if mf.flags(path) == "m":
            submodules.append(
                Submodule(path, "", path, True, weakref.proxy(repo)),
            )
    return submodules
