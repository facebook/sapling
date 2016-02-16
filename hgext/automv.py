# automv.py
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Check for unrecorded moves at commit time (EXPERIMENTAL)

This extension checks at commit/amend time if any of the committed files
comes from an unrecorded mv.

The threshold at which a file is considered a move can be set with the
``automv.similarity`` config option. This option takes a percentage between 0
(disabled) and 100 (files must be identical), the default is 95.

"""

# Using 95 as a default similarity is based on an analysis of the mercurial
# repositories of the cpython, mozilla-central & mercurial repositories, as
# well as 2 very large facebook repositories. At 95 50% of all potential
# missed moves would be caught, as well as correspond with 87% of all
# explicitly marked moves.  Together, 80% of moved files are 95% similar or
# more.
#
# See http://markmail.org/thread/5pxnljesvufvom57 for context.

from __future__ import absolute_import

from mercurial import (
    commands,
    copies,
    error,
    extensions,
    scmutil,
    similar
)
from mercurial.i18n import _

def extsetup(ui):
    entry = extensions.wrapcommand(
        commands.table, 'commit', mvcheck)
    entry[1].append(
        ('', 'no-automv', None,
         _('disable automatic file move detection')))

def mvcheck(orig, ui, repo, *pats, **opts):
    """Hook to check for moves at commit time"""
    renames = None
    disabled = opts.pop('no_automv', False)
    if not disabled:
        threshold = ui.configint('automv', 'similarity', 95)
        if not 0 <= threshold <= 100:
            raise error.Abort(_('automv.similarity must be between 0 and 100'))
        if threshold > 0:
            match = scmutil.match(repo[None], pats, opts)
            added, removed = _interestingfiles(repo, match)
            renames = _findrenames(repo, match, added, removed,
                                   threshold / 100.0)

    with repo.wlock():
        if renames is not None:
            scmutil._markchanges(repo, (), (), renames)
        return orig(ui, repo, *pats, **opts)

def _interestingfiles(repo, matcher):
    """Find what files were added or removed in this commit.

    Returns a tuple of two lists: (added, removed). Only files not *already*
    marked as moved are included in the added list.

    """
    stat = repo.status(match=matcher)
    added = stat[1]
    removed = stat[2]

    copy = copies._forwardcopies(repo['.'], repo[None], matcher)
    # remove the copy files for which we already have copy info
    added = [f for f in added if f not in copy]

    return added, removed

def _findrenames(repo, matcher, added, removed, similarity):
    """Find what files in added are really moved files.

    Any file named in removed that is at least similarity% similar to a file
    in added is seen as a rename.

    """
    renames = {}
    if similarity > 0:
        for src, dst, score in similar.findrenames(
                repo, added, removed, similarity):
            if repo.ui.verbose:
                repo.ui.status(
                    _('detected move of %s as %s (%d%% similar)\n') % (
                        matcher.rel(src), matcher.rel(dst), score * 100))
            renames[dst] = src
    if renames:
        repo.ui.status(_('detected move of %d files\n') % len(renames))
    return renames
