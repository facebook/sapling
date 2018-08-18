# inhibit.py - redefine bumped(), divergent() revsets
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""redefine obsolete(), bumped(), divergent() revsets"""

from __future__ import absolute_import

from mercurial import error, extensions, obsolete, util


revive = obsolete.revive


def uisetup(ui):
    revsets = obsolete.cachefuncs

    # make divergent() and bumped() empty
    # NOTE: we should avoid doing this but just change templates to only show a
    # subset of troubles we care about.
    revsets["divergent"] = revsets["bumped"] = lambda repo: frozenset()
