# commitextras.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''adds a new flag extras to commit (ADVANCED)'''

from __future__ import absolute_import

import re

from mercurial.i18n import _
from mercurial import (
    commands,
    error,
    extensions,
    registrar,
)

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = 'ships-with-hg-core'

usedinternally = {
    'amend_source',
    'branch',
    'close',
    'histedit_source',
    'topic',
    'rebase_source',
    'intermediate-source',
    '__touch-noise__',
    'source',
    'transplant_source',
}

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'commit', _commit)
    options = entry[1]
    options.append(('', 'extra', [],
        _('set a changeset\'s extra values'), _("KEY=VALUE")))

def _commit(orig, ui, repo, *pats, **opts):
    origcommit = repo.commit
    try:
        def _wrappedcommit(*innerpats, **inneropts):
            extras = opts.get(r'extra')
            if extras:
                for raw in extras:
                    if '=' not in raw:
                        msg = _("unable to parse '%s', should follow "
                                "KEY=VALUE format")
                        raise error.Abort(msg % raw)
                    k, v = raw.split('=', 1)
                    if not k:
                        msg = _("unable to parse '%s', keys can't be empty")
                        raise error.Abort(msg % raw)
                    if re.search('[^\w-]', k):
                        msg = _("keys can only contain ascii letters, digits,"
                                " '_' and '-'")
                        raise error.Abort(msg)
                    if k in usedinternally:
                        msg = _("key '%s' is used internally, can't be set "
                                "manually")
                        raise error.Abort(msg % k)
                    inneropts[r'extra'][k] = v
            return origcommit(*innerpats, **inneropts)

        # This __dict__ logic is needed because the normal
        # extension.wrapfunction doesn't seem to work.
        repo.__dict__['commit'] = _wrappedcommit
        return orig(ui, repo, *pats, **opts)
    finally:
        del repo.__dict__['commit']
