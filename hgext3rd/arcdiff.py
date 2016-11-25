# arcdiff.py - extension adding an option to the diff command to show changes
#              since the last arcanist diff
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import commands, error, extensions
from mercurial.i18n import _

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

def _differentialhash(ui, repo, phabrev):
    client = conduit.Client()
    try:
        client.apply_arcconfig(arcconfig.load_for_path(repo.root))

        diffid = diffprops.getcurrentdiffidforrev(client, phabrev)
        if diffid is None:
            return None

        localcommits = diffprops.getlocalcommitfordiffid(client, diffid)
        return localcommits.get('commit', None) if localcommits else None
    except conduit.ClientError as e:
        ui.warn(_('Error calling conduit: %s\n') % str(e))
        return None
    except arcconfig.ArcConfigError as e:
        raise error.Abort(str(e))

def _diff(orig, ui, repo, *pats, **opts):
    if not opts.get('since_last_arc_diff'):
        return orig(ui, repo, *pats, **opts)

    ctx = repo['.']
    phabrev = diffprops.parserevfromcommitmsg(ctx.description())

    if phabrev is None:
        mess = _('local changeset is not associated with a differential '
                 'revision')
        raise error.Abort(mess)

    rev = _differentialhash(ui, repo, phabrev)
    if rev is None:
        mess = _('unable to determine previous changeset hash')
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
