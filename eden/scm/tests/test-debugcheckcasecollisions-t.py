# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "newrepo"
sh % "mkdir -p dirA/subdirA dirA/subdirB dirB"
sh % "touch dirA/subdirA/file1 dirA/subdirB/file2 dirB/file3 file4"
sh % "hg commit -Aqm base"

# Check basic case collisions
sh % "hg debugcheckcasecollisions DIRA/subdira/FILE1 DIRA/SUBDIRB/file2 DIRB/FILE3" == r"""
    DIRA/subdira/FILE1 conflicts with dirA/subdirA/file1
    DIRA/subdira (directory for DIRA/subdira/FILE1) conflicts with dirA/subdirA (directory for dirA/subdirA/file1)
    DIRA (directory for DIRA/SUBDIRB/file2) conflicts with dirA (directory for dirA/subdirA/file1)
    DIRA/SUBDIRB/file2 conflicts with dirA/subdirB/file2
    DIRA/SUBDIRB (directory for DIRA/SUBDIRB/file2) conflicts with dirA/subdirB (directory for dirA/subdirB/file2)
    DIRB/FILE3 conflicts with dirB/file3
    DIRB (directory for DIRB/FILE3) conflicts with dirB (directory for dirB/file3)
    [1]"""

# Check a dir that collides with a file
sh % "hg debugcheckcasecollisions FILE4/foo" == r"""
    FILE4 (directory for FILE4/foo) conflicts with file4
    [1]"""

# Check a file that collides with a dir
sh % "hg debugcheckcasecollisions DIRb" == r"""
    DIRb conflicts with dirB (directory for dirB/file3)
    [1]"""

# Check self-conflicts
sh % "hg debugcheckcasecollisions newdir/newfile NEWdir/newfile newdir/NEWFILE" == r"""
    NEWdir/newfile conflicts with newdir/newfile
    NEWdir (directory for NEWdir/newfile) conflicts with newdir (directory for newdir/newfile)
    newdir/NEWFILE conflicts with newdir/newfile
    [1]"""

# Check against a particular revision
sh % "hg debugcheckcasecollisions -r 0 FILE4" == r"""
    FILE4 conflicts with file4
    [1]"""
