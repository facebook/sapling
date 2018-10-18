# perftweaks.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extension for tweaking Mercurial features to improve performance.

::
    [perftweaks]
    # Whether to use faster hidden cache. It has faster cache hash calculation
    # which only check stat of a few files inside store/ directory.
    fasthiddencache = False
"""

testedwith = "ships-with-fb-hgext"


def reposetup(ui, repo):
    if repo.local() is not None:
        # record nodemap lag
        try:
            lag = repo.changelog.nodemap.lag
            ui.log("nodemap_lag", "", nodemap_lag=lag)
        except AttributeError:
            pass
