# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from typing import Iterable, List

from . import error, extensions
from .ext.extlib.phabricator import graphql
from .i18n import _
from .node import bin, hex
from .util import timer


BATCH_SIZE = 500

FASTLOG_MAX = 100
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
            batch_size=BATCH_SIZE,
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
    batch_size=FASTLOG_MAX,
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
