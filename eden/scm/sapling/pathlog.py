# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from typing import Iterable, List

from .util import timer


BATCH_SIZE = 500


@dataclass
class PathlogStats:
    total_time: int = 0
    history_time: int = 0
    prefetch_commit_time: int = 0
    prefetch_commit_text_time: int = 0


def pathlog(repo, node, path, is_prefetch_commit_text=False) -> Iterable[bytes]:
    return strategy_pathhisotry(
        repo,
        node,
        path,
        batch_size=BATCH_SIZE,
        is_prefetch_commit_text=is_prefetch_commit_text,
        stats=PathlogStats(),
    )


def strategy_pathhisotry(
    repo, node, path, batch_size, is_prefetch_commit_text=False, stats=None
) -> List[bytes]:
    dag = repo.changelog.dag
    start_time = int(timer())
    hist = repo.pathhistory([path], dag.ancestors([node]))
    while True:
        nodes = []

        loop_start_time = int(timer())

        while len(nodes) < batch_size:
            try:
                nodes.append(next(hist))
            except StopIteration:
                break

        batch_end_time = int(timer())
        if stats:
            stats.history_time += batch_end_time - loop_start_time

        if is_prefetch_commit_text:
            # prefetch commit texts.
            repo.changelog.inner.getcommitrawtextlist(nodes)
            prefetch_commit_text_end_time = int(timer())
            if stats:
                stats.prefetch_commit_text_time += (
                    prefetch_commit_text_end_time - batch_end_time
                )

        if stats:
            stats.total_time = int(timer()) - start_time
            repo.ui.note(f"pathlog stats for '{path}': {stats}\n")

        yield from nodes

        if len(nodes) < batch_size:
            break
