# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import scmutil

from .pullrequeststore import PullRequestStore


def follow(_ui, repo, *revs):
    pr_store = PullRequestStore(repo)
    revs = set(scmutil.revrange(repo, revs))
    nodes = [repo[r].node() for r in revs]
    pr_store.follow_all(nodes)
