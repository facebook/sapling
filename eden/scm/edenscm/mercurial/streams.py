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
    # streamcommitrawtext will turn a Python iterator to a Rust Stream in a
    # background thread. Multiple threads might try to obtain
    # async_runtime::block_on_exclusive lock and cause deadlock. Upgrading
    # tokio would allow us to block_on without taking &mut Runtime and avoid
    # deadlocks.
    #
    # Do not streamcommitrawtext it for now.
    return ctxstream
