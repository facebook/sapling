# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Make sure the sparse extension does not break functionality when it gets
# loaded in a non-sparse repository.

# First create a base repository with sparse enabled.

sh % "hg init base"
sh % "cd base"
sh % "cat" << r"""
[extensions]
sparse=
journal=
""" > ".hg/hgrc"

sh % "echo a" > "file1"
sh % "echo x" > "file2"
sh % "hg ci -Aqm initial"
sh % "cd .."

# Now create a shared working copy that is not sparse.

sh % "hg --config 'extensions.share=' share base shared" == r"""
    updating working directory
    2 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd shared"
sh % "cat" << r"""
[extensions]
share=
sparse=!
journal=
""" > ".hg/hgrc"

# Make sure "hg diff" works in the non-sparse working directory.

sh % "echo z" >> "file1"
sh % "hg diff" == r"""
    diff -r 1f02e070b36e file1
    --- a/file1	Thu Jan 01 00:00:00 1970 +0000
    +++ b/file1	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,2 @@
     a
    +z"""
