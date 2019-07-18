# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"
sh % "echo foo" > "a"
sh % "hg add a"
sh % "hg commit -m 1"

sh % "echo bar" > "b"
sh % "hg add b"
sh % "hg remove a"

# Should show a removed and b added:

sh % "hg status" == r"""
    A b
    R a"""

sh % "hg revert --all" == r"""
    undeleting a
    forgetting b"""

# Should show b unknown and a back to normal:

sh % "hg status" == "? b"

sh % "rm b"

sh % "hg co -C 0" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo foo-a" > "a"
sh % "hg commit -m 2a"

sh % "hg co -C 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo foo-b" > "a"
sh % "hg commit -m 2b"

sh % "'HGMERGE=true' hg merge 1" == r"""
    merging a
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

# Should show foo-b:

sh % "cat a" == "foo-b"

sh % "echo bar" > "b"
sh % "hg add b"
sh % "rm a"
sh % "hg remove a"

# Should show a removed and b added:

sh % "hg status" == r"""
    A b
    R a"""

# Revert should fail:

sh % "hg revert" == r"""
    abort: uncommitted merge with no revision specified
    (use 'hg update' or see 'hg help revert')
    [255]"""

# Revert should be ok now:

sh % "hg revert -r2 --all" == r"""
    undeleting a
    forgetting b"""

# Should show b unknown and a marked modified (merged):

sh % "hg status" == r"""
    M a
    ? b"""

# Should show foo-b:

sh % "cat a" == "foo-b"
