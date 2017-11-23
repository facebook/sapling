# rewriteutil.py - utility functions for rewriting changesets
#
# Copyright 2017 Octobus <contact@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import (
    obsolete,
    revset,
)

def disallowednewunstable(repo, revs):
    """Checks whether editing the revs will create new unstable changesets and
    are we allowed to create them.

    To allow new unstable changesets, set the config:
        `experimental.evolution.allowunstable=True`
    """
    allowunstable = obsolete.isenabled(repo, obsolete.allowunstableopt)
    if allowunstable:
        return revset.baseset()
    return repo.revs("(%ld::) - %ld", revs, revs)
