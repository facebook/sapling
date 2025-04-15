# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""predefined hooks"""

import bindings

# Try to avoid module-level imports so this module has low-overhead if run by
# Rust hook handling.


def backgroundfsync(repo=None, **kwargs) -> None:
    """run fsync in background

    Example config::

        [hooks]
        postwritecommand.fsync = python:sapling.hooks.backgroundfsync
    """
    if not repo:
        return

    from . import util

    util.spawndetached(util.hgcmd() + ["debugfsync"], cwd=repo.store_path)


def edenfs_redirect_fixup(repo, **kwargs) -> None:
    bindings.checkout.edenredirectfixup(repo.config, repo.workingcopy())
