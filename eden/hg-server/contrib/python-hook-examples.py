# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""
Examples of useful python hooks for Mercurial.
"""
from __future__ import absolute_import

from edenscm.mercurial import patch, util


def diffstat(ui, repo, **kwargs):
    """Example usage:

    [hooks]
    commit.diffstat = python:/path/to/this/file.py:diffstat
    changegroup.diffstat = python:/path/to/this/file.py:diffstat
    """
    if kwargs.get("parent2"):
        return
    node = kwargs["node"]
    first = repo[node].p1().node()
    if "url" in kwargs:
        last = repo["tip"].node()
    else:
        last = node
    diff = patch.diff(repo, first, last)
    ui.write(patch.diffstat(util.iterlines(diff)))
