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
``automv.similarity`` config option; the default value is 1.00.

"""
from __future__ import absolute_import

from mercurial import (
    commands,
    copies,
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
    disabled = opts.pop('no_automv', False)
    if not disabled:
        threshold = float(ui.config('automv', 'similarity', '1.00'))
        if threshold > 0:
            match = scmutil.match(repo[None], pats, opts)
            added, removed = _interestingfiles(repo, match)
            renames = _findrenames(repo, match, added, removed, threshold)
            _markchanges(repo, renames)

    # developer config: automv.testmode
    if not ui.configbool('automv', 'testmode'):
        return orig(ui, repo, *pats, **opts)

def _interestingfiles(repo, matcher):
    stat = repo.status(repo['.'], repo[None], matcher)
    added = stat[1]
    removed = stat[2]

    copy = copies._forwardcopies(repo['.'], repo[None], matcher)
    # remove the copy files for which we already have copy info
    added = [f for f in added if f not in copy]

    return added, removed

def _findrenames(repo, matcher, added, removed, similarity):
    """Find renames from removed files of the current commit/amend files
    to the added ones"""
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

def _markchanges(repo, renames):
    """Marks the files in renames as copied."""
    wctx = repo[None]
    wlock = repo.wlock()
    try:
        for dst, src in renames.iteritems():
            wctx.copy(src, dst)
    finally:
        wlock.release()
