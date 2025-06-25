# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from typing import Iterable, List

from . import error

from .ext.extlib.phabricator import graphql
from .i18n import _
from .node import bin, hex
from .util import timer


BATCH_SIZE = 500

FASTLOG_MAX = 100
FASTLOG_QUEUE_SIZE = 1000
FASTLOG_TIMEOUT = 50


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


class FastLog:
    """Class which talks to a remote SCMQuery

    We page results in windows of up to FASTLOG_MAX to avoid generating
    too many results; this has been optimized on the server to cache
    fast continuations but this assumes service stickiness.

    * reponame - repository name (str)
    * scm - scm type (str)
    * start_node - node to start logging from
    * path - path to request logs
    * repo - mercurial repository object
    """

    def __init__(self, reponame, scm, node, path, repo):
        self.reponame = reponame
        self.scm = scm
        self.start_node = node
        self.path = path
        self.repo = repo
        self.ui = repo.ui

    def gettodo(self):
        return FASTLOG_MAX

    def generate_nodes(self):
        path = self.path
        start_hex = hex(self.start_node)
        reponame = self.reponame
        skip = 0
        usemutablehistory = self.ui.configbool("fastlog", "followmutablehistory")

        while True:
            results = None
            todo = self.gettodo()
            client = graphql.Client(repo=self.repo)
            results = client.scmquery_log(
                reponame,
                self.scm,
                start_hex,
                file_paths=[path],
                skip=skip,
                number=todo,
                use_mutable_history=usemutablehistory,
                timeout=FASTLOG_TIMEOUT,
            )

            if results is None:
                raise error.Abort(_("ScmQuery fastlog returned nothing unexpectedly"))

            server_nodes = [bin(commit["hash"]) for commit in results]

            # `filternodes` has a desired side effect that fetches nodes
            # (in lazy changelog) in batch.
            nodes = self.repo.changelog.filternodes(server_nodes)
            if len(nodes) != len(server_nodes):
                missing_nodes = set(server_nodes) - set(nodes)
                self.repo.ui.status_err(
                    _("fastlog: server returned extra nodes unknown locally: %s\n")
                    % " ".join(sorted([hex(n) for n in missing_nodes]))
                )
            yield from nodes

            skip += todo
            if len(results) < todo:
                break
