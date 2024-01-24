# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import importlib
import sys
from ctypes import ArgumentError


class MercurialImporter:
    """
    Intercept legacy imports using "edenscm", "mercurial", or "edenscm.mercurial" and
    resolve with import of sapling$1.

    This provides compatibility to legacy hooks and extensions that still use old imports.
    """

    legacy_names = sorted(
        ["edenscm.mercurial", "mercurial", "edenscm"], key=len, reverse=True
    )
    legacy_names_set = set(legacy_names)
    legacy_prefixes = [ln + "." for ln in legacy_names]

    # This implements the "Finder" interface.
    def find_module(self, fullname, _path):
        if fullname in self.legacy_names_set or any(
            fullname.startswith(lp) for lp in self.legacy_prefixes
        ):
            return self

        return None

    # This implements the "Loader" interface.
    def load_module(self, fullname):
        for ln in self.legacy_names:
            if fullname.startswith(ln):
                realname = "sapling" + fullname[len(ln) :]
                break
        else:
            raise ArgumentError(
                "MercurialImporter.load_module used for non-legacy module %s" % fullname
            )

        mod = importlib.import_module(realname)
        sys.modules[fullname] = mod
        return mod
