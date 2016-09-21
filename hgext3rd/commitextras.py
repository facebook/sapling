# commitextras.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import util, cmdutil, commands, hg, scmutil
from mercurial import bookmarks, extensions
from mercurial.i18n import _
from hgext import rebase
import errno, os, stat, subprocess

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'commit', _commit)
    options = entry[1]
    options.append(('', 'extra', [],
        _('set a changeset\'s extra values'), _("KEY=VALUE")))

def _commit(orig, ui, repo, *pats, **opts):
    origcommit = repo.commit
    try:
        def _wrappedcommit(*innerpats, **inneropts):
            extras = opts.get('extra')
            if extras:
                for raw in extras:
                    k, v = raw.split('=', 1)
                    inneropts['extra'][k] = v
            return origcommit(*innerpats, **inneropts)

        # This __dict__ logic is needed because the normal
        # extension.wrapfunction doesn't seem to work.
        repo.__dict__['commit'] = _wrappedcommit
        return orig(ui, repo, *pats, **opts)
    finally:
        del repo.__dict__['commit']
