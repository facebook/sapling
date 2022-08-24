# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""predefined hooks"""

from . import util


def backgroundfsync(ui, repo, hooktype, **kwargs):
    """run fsync in background

    Example config::

        [hooks]
        postwritecommand.fsync = python:edenscm.hooks.backgroundfsync
    """
    if not repo:
        return
    util.spawndetached(util.hgcmd() + ["debugfsync"], cwd=repo.svfs.join(""))
