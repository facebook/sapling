#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif
  $ newrepo
  $ mkdir -p dirA/subdirA dirA/subdirB dirB
  $ touch dirA/subdirA/file1 dirA/subdirB/file2 dirB/file3 file4
  $ hg commit -Aqm base

# Check basic case collisions

  $ hg debugcheckcasecollisions DIRA/subdira/FILE1 DIRA/SUBDIRB/file2 DIRB/FILE3
  DIRA (directory for DIRA/SUBDIRB/file2) conflicts with dirA
  DIRB (directory for DIRB/FILE3) conflicts with dirB
  [1]

# Check a dir that collides with a file

  $ hg debugcheckcasecollisions FILE4/foo
  FILE4 (directory for FILE4/foo) conflicts with file4
  [1]

# Check a file that collides with a dir

  $ hg debugcheckcasecollisions DIRb
  DIRb conflicts with dirB
  [1]

# Check self-conflicts

  $ hg debugcheckcasecollisions newdir/newfile NEWdir/newfile newdir/NEWFILE
  NEWdir/newfile conflicts with newdir/newfile
  NEWdir (directory for NEWdir/newfile) conflicts with newdir (directory for newdir/newfile)
  newdir/NEWFILE conflicts with newdir/newfile
  [1]

# Check against a particular revision

  $ hg debugcheckcasecollisions -r 0 FILE4
  FILE4 conflicts with file4
  [1]
