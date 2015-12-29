# automv.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
This extension checks at commit/amend time if any of the committed files
comes from an unrecorded mv
"""

from mercurial import cmdutil, scmutil
from mercurial import similar, util, commands, copies
from mercurial.extensions import wrapcommand
from mercurial import extensions
from mercurial.i18n import _

def extsetup(ui):
    entry = wrapcommand(commands.table, 'commit', mvcheck)
    entry[1].append(('', 'no-move-detection', None,
         _('disable automatic' + 'file move detection')))
    try:
        module = extensions.find('fbamend')
        entry = wrapcommand(module.cmdtable, 'amend', mvcheck)
        entry[1].append(('', 'no-move-detection', None,
             _('disable automatic' + 'file move detection')))
    except KeyError:
        pass

def mvcheck(orig, ui, repo, *pats, **opts):
    if not opts.get('no_move_detection'):
        threshold = float(ui.config('automv', 'similaritythres', '0.75'))
        if threshold > 0:
            match = scmutil.match(repo[None], pats, opts)
            added, removed = _interestingfiles(repo, match)
            renames = _findrenames(repo, match, added, removed, threshold)
            _markchanges(repo, renames)
    if 'no_move_detection' in opts:
        del opts['no_move_detection']

    if ui.configbool('automv', 'testmode'):
        return
    else:
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
        for src, dst, score in similar.findrenames(repo, added, removed,
                                                   similarity):
            if repo.ui.verbose:
                repo.ui.status(_('detected move of %s as %s (%d%% similar)\n')
                          % (matcher.rel(src), matcher.rel(dst), score * 100))
            renames[dst] = src
    n = len(renames)
    if n == 1:
        repo.ui.status(_('detected move of 1 file\n'))
    elif n > 1:
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


