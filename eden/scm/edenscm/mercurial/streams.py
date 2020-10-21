# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from typing import Iterable

import edenscm  # noqa: F401

from .node import wdirid


def prefetchtextstream(repo, ctxstream):
    # type: (edenscm.mercurial.localrepo.localrepository, Iterable[edenscm.mercurial.context.basectx]) -> Iterable[edenscm.mercurial.context.basectx]
    """Prefetch commit text for a stream of ctx"""

    if not repo.changelog.userust():
        # Non-Rust changelog is not lazy and does not need prefetch.
        return ctxstream
    else:
        return _prefetchtextstream(repo, ctxstream)


def _prefetchtextstream(repo, ctxstream):
    def rewritenone(n):
        # None is used as a way to refer to "working parent", ex. `repo[None]`.
        # Rust bindings do not like None. Rewrite it to `wdirid`.
        if n is None:
            return wdirid
        else:
            return n

    # NOTE: This _might_ be optimized to be zero-cost Python <-> Rust,
    # by providing extra hints about Rust nameset object.
    nodestream = (rewritenone(ctx.node()) for ctx in ctxstream)
    for node, text in repo.changelog.inner.streamcommitrawtext(nodestream):
        ctx = repo[node]
        ctx._text = text
        yield ctx
