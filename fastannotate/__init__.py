# Copyright 2016-present Facebook. All Rights Reserved.
#
# fastannotate: faster annotate implementation using linelog
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


"""yet another annotate implementation that might be faster

The fastannotate extension provides a 'fastannotate' command that makes
use of the linelog data structure as a cache layer and is expected to
be faster than the vanilla 'annotate' if the cache is present.

::

    [fastannotate]
    # specify the main branch head. the internal linelog will only contain
    # the linear (ignoring p2) "mainbranch". since linelog cannot move
    # backwards without a rebuild, this should be something that always moves
    # forward, usually it is "master" or "@".
    mainbranch = master
    # add a "fastannotate" command, and replace the default "annotate" command
    commands = fastannotate, annotate
    # use unfiltered repo for better performance
    unfilteredrepo = True
    # sacrifice correctness in some cases for performance (default: False)
    perfhack = True
"""

from __future__ import absolute_import

from fastannotate import commands

from mercurial import (
    cmdutil,
    error as hgerror,
)

testedwith = 'internal'

cmdtable = {}
command = cmdutil.command(cmdtable)

def uisetup(ui):
    cmdnames = ui.configlist('fastannotate', 'commands', ['fastannotate'])
    for name in set(cmdnames):
        if name == 'fastannotate':
            command('^fastannotate|fastblame|fa',
                    **commands.fastannotatecommandargs
                   )(commands.fastannotate)
        elif name == 'annotate':
            commands.replacedefault()
        else:
            raise hgerror.Abort(_('%s: invalid fastannotate.commands option')
                                % name)
