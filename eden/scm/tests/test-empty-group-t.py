# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
#  A          B
#
#  3  4       3
#  |\/|       |\
#  |/\|       | \
#  1  2       1  2
#  \ /        \ /
#   0          0
#
# if the result of the merge of 1 and 2
# is the same in 3 and 4, no new manifest
# will be created and the manifest group
# will be empty during the pull
#
# (plus we test a failure where outgoing
# wrongly reported the number of csets)

sh % "hg init a"
sh % "cd a"
sh % "touch init"
sh % "hg ci -A -m 0" == "adding init"
sh % "touch x y"
sh % "hg ci -A -m 1" == r"""
    adding x
    adding y"""

sh % "hg update 0" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "touch x y"
sh % "hg ci -A -m 2" == r"""
    adding x
    adding y"""

sh % "hg merge 1" == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg ci -A -m m1"

sh % "hg update -C 1" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg merge 2" == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg ci -A -m m2"

sh % "cd .."

sh % "hg clone -r 3 a b" == r"""
    adding changesets
    adding manifests
    adding file changes
    added 4 changesets with 3 changes to 3 files
    new changesets 5fcb73622933:d15a0c284984
    updating to branch default
    3 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

sh % "hg clone -r 4 a c" == r"""
    adding changesets
    adding manifests
    adding file changes
    added 4 changesets with 3 changes to 3 files
    new changesets 5fcb73622933:1ec3c74fc0e0
    updating to branch default
    3 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

sh % "hg -R a outgoing b" == r"""
    comparing with b
    searching for changes
    changeset:   4:1ec3c74fc0e0
    tag:         tip
    parent:      1:79f9e10cd04e
    parent:      2:8e1bb01c1a24
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     m2"""
sh % "hg -R a outgoing c" == r"""
    comparing with c
    searching for changes
    changeset:   3:d15a0c284984
    parent:      2:8e1bb01c1a24
    parent:      1:79f9e10cd04e
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     m1"""
sh % "hg -R b outgoing c" == r"""
    comparing with c
    searching for changes
    changeset:   3:d15a0c284984
    tag:         tip
    parent:      2:8e1bb01c1a24
    parent:      1:79f9e10cd04e
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     m1"""
sh % "hg -R c outgoing b" == r"""
    comparing with b
    searching for changes
    changeset:   3:1ec3c74fc0e0
    tag:         tip
    parent:      1:79f9e10cd04e
    parent:      2:8e1bb01c1a24
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     m2"""

sh % "hg -R b pull a" == r"""
    pulling from a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 0 files
    new changesets 1ec3c74fc0e0"""

sh % "hg -R c pull a" == r"""
    pulling from a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 0 files
    new changesets d15a0c284984"""
