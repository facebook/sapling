# pager.py - display output using a pager
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# To load the extension, add it to your configuration file:
#
#   [extension]
#   pager =
#
# Run 'hg help pager' to get info on configuration.

'''browse command output with an external pager (DEPRECATED)

Forcibly enable paging for individual commands that don't typically
request pagination with the attend-<command> option. This setting
takes precedence over ignore options and defaults::

  [pager]
  attend-cat = false
'''
from __future__ import absolute_import

from mercurial import (
    cmdutil,
    commands,
    dispatch,
    extensions,
    )

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def uisetup(ui):

    def pagecmd(orig, ui, options, cmd, cmdfunc):
        auto = options['pager'] == 'auto'
        if auto and not ui.pageractive:
            usepager = False
            attend = ui.configlist('pager', 'attend', attended)
            ignore = ui.configlist('pager', 'ignore')
            cmds, _ = cmdutil.findcmd(cmd, commands.table)

            for cmd in cmds:
                var = 'attend-%s' % cmd
                if ui.config('pager', var):
                    usepager = ui.configbool('pager', var)
                    break
                if (cmd in attend or
                     (cmd not in ignore and not attend)):
                    usepager = True
                    break

            if usepager:
                # Slight hack: the attend list is supposed to override
                # the ignore list for the pager extension, but the
                # core code doesn't know about attend, so we have to
                # lobotomize the ignore list so that the extension's
                # behavior is preserved.
                ui.setconfig('pager', 'ignore', '', 'pager')
                ui.pager('extension-via-attend-' + cmd)
        return orig(ui, options, cmd, cmdfunc)

    # Wrap dispatch._runcommand after color is loaded so color can see
    # ui.pageractive. Otherwise, if we loaded first, color's wrapped
    # dispatch._runcommand would run without having access to ui.pageractive.
    def afterloaded(loaded):
        extensions.wrapfunction(dispatch, '_runcommand', pagecmd)
    extensions.afterloaded('color', afterloaded)

attended = [
    'the-default-attend-list-is-now-empty-but-that-breaks-the-extension',
]
