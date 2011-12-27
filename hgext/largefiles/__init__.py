# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''track large binary files

Large binary files tend to be not very compressible, not very
diffable, and not at all mergeable. Such files are not handled
efficiently by Mercurial's storage format (revlog), which is based on
compressed binary deltas; storing large binary files as regular
Mercurial files wastes bandwidth and disk space and increases
Mercurial's memory usage. The largefiles extension addresses these
problems by adding a centralized client-server layer on top of
Mercurial: largefiles live in a *central store* out on the network
somewhere, and you only fetch the revisions that you need when you
need them.

largefiles works by maintaining a "standin file" in .hglf/ for each
largefile. The standins are small (41 bytes: an SHA-1 hash plus
newline) and are tracked by Mercurial. Largefile revisions are
identified by the SHA-1 hash of their contents, which is written to
the standin. largefiles uses that revision ID to get/put largefile
revisions from/to the central store. This saves both disk space and
bandwidth, since you don't need to retrieve all historical revisions
of large files when you clone or pull.

To start a new repository or add new large binary files, just add
--large to your :hg:`add` command. For example::

  $ dd if=/dev/urandom of=randomdata count=2000
  $ hg add --large randomdata
  $ hg commit -m 'add randomdata as a largefile'

When you push a changeset that adds/modifies largefiles to a remote
repository, its largefile revisions will be uploaded along with it.
Note that the remote Mercurial must also have the largefiles extension
enabled for this to work.

When you pull a changeset that affects largefiles from a remote
repository, Mercurial behaves as normal. However, when you update to
such a revision, any largefiles needed by that revision are downloaded
and cached (if they have never been downloaded before). This means
that network access may be required to update to changesets you have
not previously updated to.

If you already have large files tracked by Mercurial without the
largefiles extension, you will need to convert your repository in
order to benefit from largefiles. This is done with the
:hg:`lfconvert` command::

  $ hg lfconvert --size 10 oldrepo newrepo

In repositories that already have largefiles in them, any new file
over 10MB will automatically be added as a largefile. To change this
threshold, set ``largefiles.minsize`` in your Mercurial config file
to the minimum size in megabytes to track as a largefile, or use the
--lfsize option to the add command (also in megabytes)::

  [largefiles]
  minsize = 2

  $ hg add --lfsize 2

The ``largefiles.patterns`` config option allows you to specify a list
of filename patterns (see :hg:`help patterns`) that should always be
tracked as largefiles::

  [largefiles]
  patterns =
    *.jpg
    re:.*\.(png|bmp)$
    library.zip
    content/audio/*

Files that match one of these patterns will be added as largefiles
regardless of their size.

The ``largefiles.minsize`` and ``largefiles.patterns`` config options
will be ignored for any repositories not already containing a
largefile. To add the first largefile to a repository, you must
explicitly do so with the --large flag passed to the :hg:`add`
command.
'''

from mercurial import commands

import lfcommands
import reposetup
import uisetup

reposetup = reposetup.reposetup
uisetup = uisetup.uisetup

commands.norepo += " lfconvert"

cmdtable = lfcommands.cmdtable
