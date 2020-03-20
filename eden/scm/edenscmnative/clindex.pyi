# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import localrepo, ui as uimod

def uisetup(ui: uimod.ui) -> None: ...
def reposetup(ui: uimod.ui, repo: localrepo.localrepository) -> None: ...
