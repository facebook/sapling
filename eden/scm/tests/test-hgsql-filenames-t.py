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
sh % "cd master"
sh % "hg log"
sh % "hg pull -q ../client"

# Verify committing odd filenames works (with % character)

sh % "hg up" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo a" > "bad%name"
sh % "hg commit -Am badname" == "adding bad%name"
sh % "echo b" > "bad%name"
sh % "hg commit -Am badname2"
