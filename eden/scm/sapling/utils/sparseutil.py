# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def shouldsparsematch(repo):
    # With the addition of edensparse, all repo objects will have
    # "sparsematch" attribute. However, we only want to run the sparse logic if
    #   1. the repo is sparse
    #   2. the repo is using edensparse
    return hasattr(repo, "sparsematch") and (
        "eden" not in repo.requirements or "edensparse" in repo.requirements
    )
