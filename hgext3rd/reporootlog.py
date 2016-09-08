# reporootlog.py - log the repo root
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""The SCM wrappers do not have a reliable and easy way of detecting the
repo root.  For instance, if someone does hg log /full/path/to/repo/content, we
cannot actually determine that /full/path/to/repo is the repo root.  We have to
somehow integrate an overly large portion of mercurial's commmand parsing
infrastrastructure to accomplish this.

However, since mercurial knows the repo root, just extract that knowledge and
send it down to the client.
"""

import os
from mercurial import (
    extensions,
    localrepo,
)
from mercurial.i18n import _

def _localrepoinit(orig, self, baseui, path=None, create=False):
    orig(self, baseui, path, create)
    kwargs = {'repo': path}
    self.ui.log("reporootlog",
                "",           # ui.log requires a format string as args[0].
                **kwargs)

def uisetup(ui):
    extensions.wrapfunction(localrepo.localrepository,
                            '__init__', _localrepoinit)
