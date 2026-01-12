# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# no-check-code

from .. import localrepo, ui
from ..node import hex
from .cmdtable import command


@command("debugedenrunpostupdatehook", [])
def edenrunpostupdatehook(ui: ui.ui, repo: localrepo.localrepository) -> None:
    """Run post-update hooks for edenfs"""
    with repo.wlock():
        parent1, parent2 = ([hex(node) for node in repo.nodes("parents()")] + ["", ""])[
            :2
        ]
        repo.hook("preupdate", throw=False, parent1=parent1, parent2=parent2)
        repo.hook("update", parent1=parent1, parent2=parent2, error=0)
