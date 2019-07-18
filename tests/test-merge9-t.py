# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

# test that we don't interrupt the merge session if
# a file-level merge failed

sh % "hg init repo"
sh % "cd repo"

sh % "echo foo" > "foo"
sh % "echo a" > "bar"
sh % "hg ci -Am 'add foo'" == r"""
    adding bar
    adding foo"""

sh % "hg mv foo baz"
sh % "echo b" >> "bar"
sh % "echo quux" > "quux1"
sh % "hg ci -Am 'mv foo baz'" == "adding quux1"

sh % "hg up -qC 0"
sh % "echo" >> "foo"
sh % "echo c" >> "bar"
sh % "echo quux" > "quux2"
sh % "hg ci -Am 'change foo'" == "adding quux2"

# test with the rename on the remote side
sh % "'HGMERGE=false' hg merge" == r"""
    merging bar
    merging foo and baz to baz
    merging bar failed!
    1 files updated, 1 files merged, 0 files removed, 1 files unresolved
    use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
    [1]"""
sh % "hg resolve -l" == r"""
    U bar
    R baz"""

# test with the rename on the local side
sh % "hg up -C 1" == "3 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "'HGMERGE=false' hg merge" == r"""
    merging bar
    merging baz and foo to baz
    merging bar failed!
    1 files updated, 1 files merged, 0 files removed, 1 files unresolved
    use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
    [1]"""

# show unresolved
sh % "hg resolve -l" == r"""
    U bar
    R baz"""

# unmark baz
sh % "hg resolve -u baz"

# show
sh % "hg resolve -l" == r"""
    U bar
    U baz"""
sh % "hg st" == r"""
    M bar
    M baz
    M quux2
    ? bar.orig"""

# re-resolve baz
sh % "hg resolve baz" == "merging baz and foo to baz"

# after resolve
sh % "hg resolve -l" == r"""
    U bar
    R baz"""

# resolve all warning
sh % "hg resolve" == r"""
    abort: no files or directories specified
    (use --all to re-merge all unresolved files)
    [255]"""

# resolve all
sh % "hg resolve -a" == r"""
    merging bar
    warning: 1 conflicts while merging bar! (edit, then use 'hg resolve --mark')
    [1]"""

# after
sh % "hg resolve -l" == r"""
    U bar
    R baz"""

sh % "cd .."
