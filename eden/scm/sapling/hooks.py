# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""predefined hooks"""

import bindings

from . import util
from .i18n import _


def backgroundfsync(ui, repo, hooktype, **kwargs) -> None:
    """run fsync in background

    Example config::

        [hooks]
        postwritecommand.fsync = python:sapling.hooks.backgroundfsync
    """
    if not repo:
        return
    util.spawndetached(util.hgcmd() + ["debugfsync"], cwd=repo.svfs.join(""))


def edenfs_redirect_fixup(ui, repo, hooktype, **kwargs) -> None:
    bindings.checkout.edenredirectfixup(ui._rcfg, repo._rsrepo.workingcopy())
