# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". '$TESTDIR/hgsql/library.sh'"
sh % "initdb"
sh % "setconfig 'extensions.treemanifest=!'"

# Populate the db with an initial commit

sh % "initclient client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "cd .."

sh % "initserver master masterrepo"
sh % "initserver master2 masterrepo"
sh % "cd master"
sh % "hg log"
sh % "hg pull -q ../client"

# Verify local commits work

sh % "hg up" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo y" > "y"
sh % "hg commit -Am y" == "adding y"

sh % "cd ../master2"
sh % "hg log -l 1" == r"""
    changeset:   1:d34c38483be9
    tag:         tip
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     y"""

# Verify local bookmarking works

sh % "hg bookmark -r 1 '@'"
sh % "hg log -r '@' --template '{rev}\\n'" == "1"
sh % "cd ../master"
sh % "hg log -r '@' --template '{rev}\\n'" == "1"
