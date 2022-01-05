# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from typing import Iterable

import edenscm  # noqa: F401

from . import util
from .node import wdirid


def prefetchtextstream(repo, ctxstream):
    # type: (edenscm.mercurial.localrepo.localrepository, Iterable[edenscm.mercurial.context.basectx]) -> Iterable[edenscm.mercurial.context.basectx]
    """Prefetch commit text for a stream of ctx"""

    return _prefetchtextstream(repo, ctxstream)


def _prefetchtextstream(repo, ctxstream):
    for ctxbatch in util.eachslice(ctxstream, 10000, maxtime=2):
        # ctxbatch: [ctx]
        nodes = [_rewritenone(c.node()) for c in ctxbatch]
        texts = repo.changelog.inner.getcommitrawtextlist(nodes)
        for ctx, text in zip(ctxbatch, texts):
            ctx._text = text
            yield ctx


def _rewritenone(n):
    # None is used as a way to refer to "working parent", ex. `repo[None]`.
    # Rust bindings do not like None. Rewrite it to `wdirid`.
    if n is None:
        return wdirid
    else:
        return n
