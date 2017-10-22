# amend.py - provide the amend command
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""provide the amend command (EXPERIMENTAL)

This extension provides an ``amend`` command that is similar to
``commit --amend`` but does not prompt an editor.
"""

from __future__ import absolute_import

from mercurial.i18n import _
from mercurial import (
    cmdutil,
    commands,
    error,
    pycompat,
    registrar,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

cmdtable = {}
command = registrar.command(cmdtable)

@command('amend',
    [('A', 'addremove', None,
      _('mark new/missing files as added/removed before committing')),
     ('e', 'edit', None, _('invoke editor on commit messages')),
     ('i', 'interactive', None, _('use interactive mode')),
     ('n', 'note', '', _('store a note on the amend')),
    ] + cmdutil.walkopts + cmdutil.commitopts + cmdutil.commitopts2,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def amend(ui, repo, *pats, **opts):
    """amend the working copy parent with all or specified outstanding changes

    Similar to :hg:`commit --amend`, but reuse the commit message without
    invoking editor, unless ``--edit`` was set.

    See :hg:`help commit` for more details.
    """
    opts = pycompat.byteskwargs(opts)
    if len(opts['note']) > 255:
        raise error.Abort(_("cannot store a note of more than 255 bytes"))
    with repo.wlock(), repo.lock():
        if not opts.get('logfile'):
            opts['message'] = opts.get('message') or repo['.'].description()
        opts['amend'] = True
        return commands._docommit(ui, repo, *pats, **pycompat.strkwargs(opts))
