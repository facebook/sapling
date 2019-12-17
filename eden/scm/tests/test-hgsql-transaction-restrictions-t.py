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

# Verify strip fails in a db repo

sh % "hg debugstrip -r tip" == r"""
    saved backup bundle to $TESTTMP/master/.hg/strip-backup/b292c1e3311f-9981e2ad-backup.hg (glob)
    transaction abort!
    rollback completed
    strip failed, backup bundle stored in '$TESTTMP/master/.hg/strip-backup/b292c1e3311f-9981e2ad-backup.hg'
    abort: invalid repo change - only hg push and pull are allowed
    [255]"""

sh % "hg log -l 1" == r"""
    changeset:   0:b292c1e3311f
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     x"""
