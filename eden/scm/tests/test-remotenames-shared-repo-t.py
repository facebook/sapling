# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Set up upstream repo

sh % "echo '[extensions]'" >> "$HGRCPATH"
sh % "echo 'share='" >> "$HGRCPATH"
sh % "echo 'remotenames='" >> "$HGRCPATH"
sh % "hg init upstream"
sh % "cd upstream"
sh % "touch file0"
sh % "hg add file0"
sh % "hg commit -m file0"
sh % "hg bookmark mainline"
sh % "cd .."

# Clone primary repo

sh % "hg clone upstream primary" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd primary"
sh % "hg log --graph" == r"""
    @  changeset:   0:d26a60f4f448
       bookmark:    default/mainline
       hoistedname: mainline
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     file0"""

# Share to secondary repo
sh % "cd .."
sh % "hg share -B primary secondary" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd secondary"
sh % "hg log --graph" == r"""
    @  changeset:   0:d26a60f4f448
       bookmark:    default/mainline
       hoistedname: mainline
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     file0"""

# Check that tracking is also shared
sh % "hg book local -t default/mainline"
sh % "hg book -v" == " * local                     0:d26a60f4f448            [default/mainline]"
sh % "cd ../primary"
sh % "hg book -v" == "   local                     0:d26a60f4f448            [default/mainline]"
