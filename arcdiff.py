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

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'diff', _diff)
    options = entry[1]
    options.append(('', 'since-last-arc-diff', None,
        _('show changes since last `arc diff`')))

def _callconduit(ui, command, params):
    try:
        process = subprocess.Popen(['arc', 'call-conduit', command],
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                    preexec_fn=os.setsid)
        input = json.dumps(params)
        process.stdin.write(input)
        process.stdin.close()
        raw = process.stdout.read()

        try:
            parsed = json.loads(raw)
            response = parsed.get('response')
            if response is None:
                ui.warn("%s\n" % parsed.get('errorMessage', 'unknown error'))
                return None

            return response
        except ValueError as e:
            ui.warn(_('unable to parse conduit response: %s\n') % str(e))
            return None

    except Exception as e:
        ui.warn(_('could not call `arc call-conduit`.\n'))
        return None

def _getlastdiff(ui, diffid):
    res = _callconduit(ui, 'differential.query', {'ids': [diffid]})
    if res is None:
        return None

    info = res[0]
    if info is None:
        return None

    diffs = info.get('diffs', [])
    return max(diffs)

def _differentialhash(ui, diffid):
    id = _getlastdiff(ui, diffid)
    if id is None:
        return None

    res = _callconduit(ui, 'differential.querydiffs', {'ids':[id]})
    if res is None:
        return None

    info = res.get(str(id))
    if info is None:
        return None

    localcommits = info.get('properties', {}).get('local:commits', {})
    if localcommits is None or len(localcommits) == 0:
        return None

    return list(localcommits.keys())[0]

def _differentialid(ctx):
    descr = ctx.description()
    match = re.search('Differential Revision: https://phabricator.fb.com/(D\d+)'
                      , descr)
    return match.group(1) if match else None

def _diff(orig, ui, repo, *pats, **opts):
    if not opts.get('since_last_arc_diff'):
        return orig(ui, repo, *pats, **opts)

    ctx = repo['.']
    diffid = _differentialid(ctx)

    if diffid is None:
        mess = _('local commit is not associated with a differential revision')
        raise error.Abort(mess)

    diffid = diffid[1:]
    rev = _differentialhash(ui, diffid)
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
