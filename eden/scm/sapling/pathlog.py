# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Iterable, List


BATCH_SIZE = 500


def pathlog(repo, node, path, is_prefetch_commit_text=False) -> Iterable[bytes]:
    return strategy_pathhisotry(
        repo,
        node,
        path,
        batch_size=BATCH_SIZE,
        is_prefetch_commit_text=is_prefetch_commit_text,
    )


def strategy_pathhisotry(
    repo, node, path, batch_size, is_prefetch_commit_text=False
) -> List[bytes]:
    dag = repo.changelog.dag
    hist = repo.pathhistory([path], dag.ancestors([node]))
    while True:
        nodes = []

        while len(nodes) < batch_size:
            try:
                nodes.append(next(hist))
            except StopIteration:
                break

        if not nodes:
            break

        if is_prefetch_commit_text:
            # prefetch commit texts.
            repo.changelog.inner.getcommitrawtextlist(nodes)

        yield from nodes

        if len(nodes) < batch_size:
            break
