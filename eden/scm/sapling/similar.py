# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# similar.py - mechanisms for finding similar files
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import bindings

from . import progress, pycompat
from .i18n import _


def _findexactmatches(repo, added, removed):
    """find renamed files that have no changes

    Takes a list of new filectxs and a list of removed filectxs, and yields
    (before, after) tuples of exact matches.
    """
    numfiles = len(added) + len(removed)

    with progress.bar(
        repo.ui, _("searching for exact renames"), _("files"), numfiles
    ) as prog:
        # Build table of removed files: {hash(fctx.data()): [fctx, ...]}.
        # We use hash() to discard fctx.data() from memory.
        hashes = {}
        for fctx in removed:
            prog.value += 1
            h = hash(fctx.data())
            if h not in hashes:
                hashes[h] = [fctx]
            else:
                hashes[h].append(fctx)

        # For each added file, see if it corresponds to a removed file.
        for fctx in added:
            prog.value += 1
            adata = fctx.data()
            h = hash(adata)
            for rfctx in hashes.get(h, []):
                # compare between actual file contents for exact identity
                if adata == rfctx.data():
                    yield (rfctx, fctx)
                    break


def _score(repo, data, otherdata, threshold):
    is_similar, score = bindings.copytrace.content_similarity(
        data, otherdata, repo.ui._rcfg, threshold
    )
    return is_similar, score


def score(fctx1, fctx2, threshold=None):
    return _score(fctx1.repo(), fctx1.data(), fctx2.data(), threshold)[1]


def _findsimilarmatches(repo, added, removed, threshold):
    """find potentially renamed files based on similar file content

    Takes a list of new filectxs and a list of removed filectxs, and yields
    (before, after, score) tuples of partial matches.
    """
    if not added or not removed:
        return None

    copies = {}
    with progress.bar(
        repo.ui, _("searching for similar files"), _("files"), len(added)
    ) as prog:
        for a in added:
            prog.value += 1
            data = a.data()
            bestscore = -1
            for r in removed:
                is_similar, score = _score(repo, data, r.data(), threshold)
                if is_similar and score > bestscore:
                    copies[a] = (r, score)
                    bestscore = score

    for dest, v in pycompat.iteritems(copies):
        source, bscore = v
        yield source, dest, bscore


def _dropempty(fctxs):
    return [x for x in fctxs if x.size() > 0]


def findrenames(repo, added, removed, threshold):
    """find renamed files -- yields (before, after, score) tuples"""
    wctx = repo[None]
    pctx = wctx.p1()

    # Zero length files will be frequently unrelated to each other, and
    # tracking the deletion/addition of such a file will probably cause more
    # harm than good. We strip them out here to avoid matching them later on.
    addedfiles = _dropempty(wctx[fp] for fp in sorted(added))
    removedfiles = _dropempty(pctx[fp] for fp in sorted(removed) if fp in pctx)

    # Find exact matches.
    matchedfiles = set()
    for a, b in _findexactmatches(repo, addedfiles, removedfiles):
        matchedfiles.add(b)
        yield (a.path(), b.path(), 1.0)

    # If the user requested similar files to be matched, search for them also.
    if threshold < 1.0:
        addedfiles = [x for x in addedfiles if x not in matchedfiles]
        for a, b, score in _findsimilarmatches(
            repo, addedfiles, removedfiles, threshold
        ):
            yield (a.path(), b.path(), score)
