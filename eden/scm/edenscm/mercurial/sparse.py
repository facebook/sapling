# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sparse.py - functionality for sparse checkouts

from __future__ import absolute_import

from . import match as matchmod


# This function is not used in mercurial any more.
# However, we still deploy a dummy implementation for now until we have finished
# rolling out updates to external modules (such as the Eden dirstate) that still
# reference this function.
def matcher(repo, revs=None, includetemp=True):
    return matchmod.always(repo.root, "")
