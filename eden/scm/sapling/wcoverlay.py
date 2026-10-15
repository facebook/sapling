# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""present file contents as the working copy without writing them to disk

Partial commits (``commit -i``, ``amend -i``) used to rewrite the working copy
to the selected content, commit, and restore the original files from backups.
An overlay instead makes working copy reads of the selected files return the
selected content, so the regular commit code paths run unchanged while the
files on disk are never touched.
"""

from __future__ import annotations

import contextlib
from typing import Dict, Iterator, Optional, Tuple

from . import error

# path -> (data, flags)
Overlay = Dict[str, Tuple[bytes, str]]


@contextlib.contextmanager
def applied(repo, files: Overlay) -> Iterator[None]:
    """make working copy reads of ``files`` return the given (data, flags)"""
    if getattr(repo, "_wcoverlay", None) is not None:
        raise error.ProgrammingError("working copy overlays cannot be nested")
    repo._wcoverlay = files
    try:
        yield
    finally:
        repo._wcoverlay = None


def lookup(repo, path: str) -> Optional[Tuple[bytes, str]]:
    """return (data, flags) if ``path`` is overlaid, otherwise None"""
    overlay = getattr(repo, "_wcoverlay", None)
    if overlay is None:
        return None
    return overlay.get(path)


def marknormal(repo, path: str) -> None:
    """mark a committed file as normal in the dirstate

    An overlaid file was committed with content that differs from the file on
    disk, so it must be re-checked by the next status instead of being
    recorded as clean.
    """
    if lookup(repo, path) is not None:
        repo.dirstate.normallookup(path)
    else:
        repo.dirstate.normal(path)
