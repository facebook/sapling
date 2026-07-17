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


def enabled_profiles(repo):
    from sapling.ext import sparse

    if not repo.localvfs.exists("sparse"):
        return set()

    raw = repo.localvfs.readutf8("sparse")
    rawconfig = sparse.readsparseconfig(repo, raw)
    return set(rawconfig.profiles)


def load_sparse_profile(repo, ctx, profile_path):
    from sapling.ext import sparse

    raw_content = sparse.getrawprofile(repo, profile_path, ctx.hex())
    raw_config = sparse.readsparseconfig(
        repo, raw_content, filename=profile_path, depth=1
    )
    matcher = sparse.computesparsematcher(repo, [ctx.rev()], raw_config)
    return raw_config, matcher


def load_sparse_profile_matcher(repo, ctx, profile_path):
    return load_sparse_profile(repo, ctx, profile_path)[1]
