# arcdiff.py - extension adding an option to the diff command to show changes
#              since the last arcanist diff
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import cmdutil, commands, error, extensions, hg, scmutil, util
from mercurial.i18n import _

import json
import os
import re
import subprocess

from phabricator import (
    arcconfig,
    conduit,
    diffprops,
)

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'diff', _diff)
    options = entry[1]
    options.append(('', 'since-last-arc-diff', None,
        _('show changes since last `arc diff`')))

def _callconduit(ui, command, params):
    try:
        return conduit.call_conduit(command, params)
    except conduit.ClientError as e:
        ui.warn(_('Error calling conduit: %s\n') % str(e))
        return None
    except arcconfig.ArcConfigError as e:
        raise error.Abort(str(e))

def _getlastdiff(ui, phabrev):
    res = _callconduit(ui, 'differential.query', {'ids': [phabrev]})
    if res is None:
        return None

    info = res[0]
    if info is None:
        return None

    diffs = info.get('diffs', [])
    if not diffs:
        return None

    return max(diffs)

def _differentialhash(ui, phabrev):
    id = _getlastdiff(ui, phabrev)
    if id is None:
        return None

    res = _callconduit(ui, 'differential.getdiffproperties', {
                       'diff_id': id,
                       'names': ['local:commits']})
    if not res:
        return None

    localcommits = res.get('local:commits', {})
    if not localcommits:
        return None

    return list(localcommits.keys())[0]

def _diff(orig, ui, repo, *pats, **opts):
    if not opts.get('since_last_arc_diff'):
        return orig(ui, repo, *pats, **opts)

    ctx = repo['.']
    phabrev = diffprops.parserevfromcommitmsg(ctx.description())

    if phabrev is None:
        mess = _('local commit is not associated with a differential revision')
        raise error.Abort(mess)

    rev = _differentialhash(ui, phabrev)
    if rev is None:
        mess = _('unable to determine previous commit hash')
        raise error.Abort(mess)

    rev = str(rev)
    opts['rev'] = [rev]

    # if patterns aren't provided, restrict diff to files in both changesets
    # this prevents performing a diff on rebased changes
    if len(pats) == 0:
        prev = set(repo.unfiltered()[rev].files())
        curr = set(repo['.'].files())
        pats = tuple(prev | curr)

    return orig(ui, repo, *pats, **opts)
