# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import importlib
import sys


class MercurialImporter(object):
    """Intercept legacy imports of edenscm.mercurial(.foo)? or mercurial(.foo)? and resolve with import of edenscm$1.

    This provides compatibility to legacy hooks and extensions that still use old imports.
    """

    # This implements the "Finder" interface.
    def find_module(self, fullname, _path):
        if (
            fullname == "edenscm.mercurial"
            or fullname.startswith("edenscm.mercurial.")
            or fullname == "mercurial"
            or fullname.startswith("mercurial.")
        ):
            return self

        return None

    # This implements the "Loader" interface.
    def load_module(self, fullname):
        if fullname.startswith("edenscm.mercurial"):
            realname = "edenscm" + fullname[17:]
        else:
            realname = "edenscm" + fullname[9:]

        mod = importlib.import_module(realname)
        sys.modules[fullname] = mod
        return mod
