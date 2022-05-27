#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ cd $TESTTMP
  $ setconfig 'format.dirstate=2'

  $ newrepo
  $ echo file1 > file1
  $ echo file2 > file2
  $ mkdir -p dira dirb
  $ echo file3 > dira/file3
  $ echo file4 > dirb/file4
  $ echo file5 > dirb/file5
  $ hg ci -q -Am base

# Test debugpathcomplete with just normal files

  $ hg debugpathcomplete f
  file1
  file2
  $ hg debugpathcomplete -f d
  dira/file3
  dirb/file4
  dirb/file5

# Test debugpathcomplete with removed files

  $ hg rm dirb/file5
  $ hg debugpathcomplete -r d
  dirb
  $ hg debugpathcomplete -fr d
  dirb/file5
  $ hg rm dirb/file4
  $ hg debugpathcomplete -n d
  dira

# Test debugpathcomplete with merges

  $ cd ..
  $ newrepo
  $ drawdag << 'EOS'
  >   D     # A/filenormal = 1
  >   |\    # B/filep1 = 1
  >   B C   # B/filemerged = 1
  >   |/    # C/filep2 = 1
  >   A     # C/filemerged = 2
  >         # D/filemerged = 12
  > EOS
  $ hg up -q $D
  $ hg debugpathcomplete f
  filemerged
  filenormal
  filep1
  filep2
