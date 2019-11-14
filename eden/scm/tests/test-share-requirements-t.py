# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["killdaemons"])

sh % "enable share"

# prepare repo1

sh % "newrepo"
sh % "echo a" > "a"
sh % "hg commit -A -q -m init"

# make a bundle we will use later

sh % "cd .."
sh % "hg -R repo1 bundle -q -a testbundle.hg"

# share it without bookmarks

sh % "hg share repo1 repo2" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# share it with bookmarks

sh % "hg share -B repo1 repo3" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# add a future store feature to repo1

sh % "echo test-futurestorefeature" > "repo1/.hg/store/requires"

# running log should fail because of the new store format feature

sh % "hg -R repo1 log -T '{node}\\n'" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo2 log -T '{node}\\n'" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo3 log -T '{node}\\n'" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""

# commands that lock the local working copy also fail correctly

sh % "hg -R repo1 co 0" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo2 co 0" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo3 co 0" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""

# commands that only lock the store also fail correctly

sh % "hg -R repo1 unbundle testbundle.hg" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo2 unbundle testbundle.hg" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
sh % "hg -R repo3 unbundle testbundle.hg" == r"""
    abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
    (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
    [255]"""
