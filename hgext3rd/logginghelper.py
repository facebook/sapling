# reporootlog.py - log the repo root
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""This extension logs different pieces of information that will be used
by SCM wrappers
"""

import os
from mercurial import (
    extensions,
    localrepo,
)

def _localrepoinit(orig, self, baseui, path=None, create=False):
    orig(self, baseui, path, create)
    reponame = self.ui.config('paths', 'default', path)
    if reponame:
        reponame = os.path.basename(reponame)
    kwargs = {'repo': reponame}
    self.ui.log("logginghelper",
                "",           # ui.log requires a format string as args[0].
                **kwargs)

def uisetup(ui):
    extensions.wrapfunction(localrepo.localrepository,
                            '__init__', _localrepoinit)
