# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from typing import Iterable

import sapling  # noqa: F401
from bindings import agentdetect

from . import error, util
from .i18n import _
from .node import wdirid


def prefetchtextstream(
    repo: "sapling.localrepo.localrepository",
    ctxstream: "Iterable[sapling.context.basectx]",
) -> "Iterable[sapling.context.basectx]":
    """Prefetch commit text for a stream of ctx"""

    return _prefetchtextstream(repo, ctxstream)


def _prefetchtextstream(repo, ctxstream):
    is_agent = agentdetect.is_agent()
    max_count = repo.ui.configint("agent", "max-commit-fetch-count", 100_000)
    batch_size = repo.ui.configint("experimental", "commit-fetch-batch-size", 10_000)
    count = 0

    for ctxbatch in util.eachslice(ctxstream, batch_size, maxtime=2):
        # ctxbatch: [ctx]
        nodes = [_rewritenone(c.node()) for c in ctxbatch]
        texts = repo.changelog.inner.getcommitrawtextlist(nodes)
        for ctx, text in zip(ctxbatch, texts):
            count += 1
            if is_agent and max_count and count > max_count:
                raise error.Abort(
                    _("revset query scanned over %d commits") % max_count,
                    hint=_("run '@prog@ help agent performance' for guidance."),
                )
            ctx._text = text
            yield ctx


def _rewritenone(n):
    # None is used as a way to refer to "working parent", ex. `repo[None]`.
    # Rust bindings do not like None. Rewrite it to `wdirid`.
    if n is None:
        return wdirid
    else:
        return n
