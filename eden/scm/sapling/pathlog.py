# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import heapq
from collections import deque
from dataclasses import dataclass
from typing import Iterable, List

from . import error, extensions
from .ext.extlib.phabricator import graphql
from .i18n import _
from .node import bin, hex
from .util import timer


BATCH_SIZE = 500

FASTLOG_BATCH_SIZE = 1000
FASTLOG_TIMEOUT = 50


@dataclass
class PathlogStats:
    total_time: int = 0
    history_time: int = 0
    prefetch_commit_time: int = 0
    prefetch_commit_text_time: int = 0


def pathlog(repo, node, path, is_prefetch_commit_text=False) -> Iterable[bytes]:
    if repo[node].ispublic() and is_fastlog_enabled(repo):
        return strategy_fastlog(
            repo,
            node,
            path,
            is_prefetch_commit_text=is_prefetch_commit_text,
            stats=PathlogStats(),
        )
    else:
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
            repo.ui.note(f"strategy_pathhisotry stats for '{path}': {stats}\n")

        yield from nodes

        if len(nodes) < batch_size:
            break


def strategy_fastlog(
    repo,
    node,
    path,
    batch_size=None,
    is_prefetch_commit_text=False,
    stats=None,
    scm_type: str = "hg",
) -> List[bytes]:
    """Fetches the history of a path from SCMQuery.

    We page results in windows of up to 'batch_size' to avoid generating
    too many results; this has been optimized on the server to cache
    fast continuations but this assumes service stickiness.
    """
    reponame = repo.ui.config("fbscmquery", "reponame")
    client = graphql.Client(repo=repo)
    start_hex = hex(node)
    skip = 0
    use_mutable_history = repo.ui.configbool("fastlog", "followmutablehistory")
    batch_size = batch_size or repo.ui.configint(
        "fastlog", "batchsize", FASTLOG_BATCH_SIZE
    )
    start_time = int(timer())

    while True:
        loop_start_time = int(timer())

        results = client.scmquery_log(
            reponame,
            scm_type,
            start_hex,
            file_paths=[path],
            skip=skip,
            number=batch_size,
            use_mutable_history=use_mutable_history,
            timeout=FASTLOG_TIMEOUT,
        )
        if results is None:
            raise error.Abort(_("ScmQuery fastlog returned nothing unexpectedly"))

        batch_end_time = int(timer())
        if stats:
            stats.history_time += batch_end_time - loop_start_time

        server_nodes = [bin(commit["hash"]) for commit in results]

        # `filternodes` has a desired side effect that fetches nodes
        # (in lazy changelog) in batch.
        nodes = repo.changelog.filternodes(server_nodes)
        prefetch_commit_end_time = int(timer())
        if stats:
            stats.prefetch_commit_time += prefetch_commit_end_time - batch_end_time

        if len(nodes) != len(server_nodes):
            missing_nodes = set(server_nodes) - set(nodes)
            repo.ui.status_err(
                _("fastlog: server returned extra nodes unknown locally: %s\n")
                % " ".join(sorted([hex(n) for n in missing_nodes]))
            )

        if is_prefetch_commit_text:
            # prefetch commit texts.
            repo.changelog.inner.getcommitrawtextlist(nodes)
            prefetch_commit_text_end_time = int(timer())
            if stats:
                stats.prefetch_commit_text_time += (
                    prefetch_commit_text_end_time - prefetch_commit_end_time
                )

        if stats:
            stats.total_time = int(timer()) - start_time
            repo.ui.note(f"strategy_fastlog stats for '{path}': {stats}\n")

        yield from nodes

        skip += batch_size
        if len(results) < batch_size:
            break


def is_fastlog_enabled(repo) -> bool:
    reponame = repo.ui.config("fbscmquery", "reponame")
    is_fastlog_ext_enabled = extensions.isenabled(repo.ui, "fastlog")
    is_fastlog_config_enabled = repo.ui.configbool("fastlog", "enabled")

    if not (reponame and is_fastlog_ext_enabled and is_fastlog_config_enabled):
        repo.ui.debug("fastlog: not used because fastlog is disabled\n")
        return False

    try:
        # Test that the GraphQL client can be constructed, to rule
        # out configuration issues like missing `.arcrc` etc.
        graphql.Client(repo=repo)
    except Exception as ex:
        repo.ui.debug(
            "fastlog: not used because graphql client cannot be constructed: %r\n" % ex
        )
        return False

    return True


def lazyparents(rev, path, public, parentfunc):
    """lazyparents(rev, path, public, parentfunc)
    Lazily yield parents of rev in reverse order until all nodes
    in public have been reached or all revs have been exhausted

    10
     | \
     9  8
     |  | \
     7  6  5
     |  | /
     4 *3   First move, 4 -3
     | /
     2 *2   Second move, 4 -1
     | *
     1

    For example:
    >>> path = "a"
    >>> parents = { 10:[9, 8], 9:[7], 8:[6,5], 7:[4], 6:[3], 5:[3], 4:[2] }
    >>> parents.update({ 3:[2], 2:[1], 1:[] })
    >>> def parentfunc(rev, curr_path):
    ...   for p in parents[rev]:
    ...     yield (p, curr_path)
    ...
    >>> public = {1}
    >>> list(lazyparents(10, path, public, parentfunc))
    [(10, 'a'), (9, 'a'), (8, 'a'), (7, 'a'), (6, 'a'), (5, 'a'), (4, 'a'), (3, 'a'), (2, 'a'), (1, 'a')]
    >>> public = {2, 3}
    >>> list(lazyparents(10, path, public, parentfunc))
    [(10, 'a'), (9, 'a'), (8, 'a'), (7, 'a'), (6, 'a'), (5, 'a'), (4, 'a'), (3, 'a'), (2, 'a')]
    >>> parents[4] = [3]
    >>> public = {3, 4, 5}
    >>> list(lazyparents(10, path, public, parentfunc))
    [(10, 'a'), (9, 'a'), (8, 'a'), (7, 'a'), (6, 'a'), (5, 'a'), (4, 'a'), (3, 'a')]
    >>> parents[4] = [1]
    >>> public = {3, 5, 7}
    >>> list(lazyparents(10, path, public, parentfunc))
    [(10, 'a'), (9, 'a'), (8, 'a'), (7, 'a'), (6, 'a'), (5, 'a'), (4, 'a'), (3, 'a'), (2, 'a'), (1, 'a')]

    5
    | \
    4  3  # 3: mv a -> b
    | /
    2
    |
    1
    >>> path = "b"
    >>> parents = {(5, "b"): [(3, "b"), (4, "a")], (4, "a"): [(2, "a")], (3, "b"): [(2, "a")], (2, "a"): [(1, "a")]}
    >>> def parentfunc(rev, curr_path):
    ...   for p, parent_path in parents[(rev, curr_path)]:
    ...     yield (p, parent_path)
    ...
    >>> public = {1}
    >>> list(lazyparents(5, path, public, parentfunc))
    [(5, 'b'), (4, 'a'), (3, 'b'), (2, 'a'), (1, 'a')]
    """
    seen = set()
    heap = [(-rev, path)]

    while heap:
        cur, cur_path = heapq.heappop(heap)
        cur = -cur
        if (cur, cur_path) not in seen:
            seen.add((cur, cur_path))
            yield (cur, cur_path)

            published = cur in public
            if published:
                # Down to one public ancestor; end generation
                if len(public) == 1:
                    return
                public.remove(cur)

            for p_rev, p_path in parentfunc(cur, cur_path):
                heapq.heappush(heap, (-p_rev, p_path))
                if published:
                    public.add(p_rev)


def findpublic(repo, rev, path, parentfunc):
    public = set()
    # Our criterion for invoking fastlog is finding a single
    # common public ancestor from the current head. First we
    # have to walk back through drafts to find all interesting
    # public parents. Typically this will just be one, but if
    # there are merged drafts, we may have multiple parents.
    if repo[rev].ispublic():
        public.add(rev)
    else:
        queue = deque()
        queue.append((rev, path))
        seen = set((rev, path))
        while queue:
            cur, cur_path = queue.popleft()
            if (cur, cur_path) not in seen:
                seen.add((cur, cur_path))
                if repo[cur].mutable():
                    for p_rev, p_path in parentfunc(cur, cur_path):
                        queue.append((p_rev, p_path))
                else:
                    public.add(cur)
    return public
