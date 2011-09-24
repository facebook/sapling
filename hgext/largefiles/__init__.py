# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''track large binary files

Large binary files tend to be not very compressible, not very "diffable", and
not at all mergeable.  Such files are not handled well by Mercurial\'s storage
format (revlog), which is based on compressed binary deltas.  largefiles solves
this problem by adding a centralized client-server layer on top of Mercurial:
largefiles live in a *central store* out on the network somewhere, and you only
fetch the ones that you need when you need them.

largefiles works by maintaining a *standin* in .hglf/ for each largefile.  The
standins are small (41 bytes: an SHA-1 hash plus newline) and are tracked by
Mercurial.  Largefile revisions are identified by the SHA-1 hash of their
contents, which is written to the standin.  largefiles uses that revision ID to
get/put largefile revisions from/to the central store.

A complete tutorial for using lfiles is included in ``usage.txt`` in the lfiles
source distribution.  See
https://developers.kilnhg.com/Repo/Kiln/largefiles/largefiles/File/usage.txt
'''

from mercurial import commands

import lfcommands
import reposetup
import uisetup

reposetup = reposetup.reposetup
uisetup = uisetup.uisetup

commands.norepo += " lfconvert"

cmdtable = lfcommands.cmdtable
