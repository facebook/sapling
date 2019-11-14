# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh


exist_dirs = ["a", "a/b", "a/b/c", "a/b/d", "b/c/", "b/d"]
non_exist_dirs = ["m", "m/n", "a/b/m", "b/m/", "b/m/n"]
all_dirs = " ".join(non_exist_dirs + exist_dirs)

for testcase in ["flat", "tree"]:

    sh % "cd $TESTTMP"

    if feature.check(["flat"]):
        sh % "setconfig 'extensions.treemanifest=!'"

    if feature.check(["tree"]):
        sh % "setconfig 'extensions.treemanifest='"

    sh % "newrepo"

    for d in exist_dirs:
        sh % ("mkdir -p %s" % d)
        sh % ("touch %s/x" % d)

    sh % "hg commit -Aqm init"

    sh % ("hg debugdirs %s" % all_dirs) == "\n".join(exist_dirs)
