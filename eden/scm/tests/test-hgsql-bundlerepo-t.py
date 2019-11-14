# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". '$TESTDIR/hgsql/library.sh'"
sh % "initdb"

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

sh % "cd .."

# Test viewing a bundle repo
sh % "cd client"
sh % "echo y" > "x"
sh % "hg commit -qAm x2"
sh % "hg bundle --base 0 --rev 1 ../mybundle.hg" == "1 changesets found"

sh % "cd ../master"
sh % "hg -R ../mybundle.hg log -r tip -T '{rev} {desc}\\n'" == "1 x2"
sh % "hg log -r tip -T '{rev} {desc}\\n'" == "0 x"
