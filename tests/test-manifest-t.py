# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Source bundle was generated with the following script:

# hg init
# echo a > a
# ln -s a l
# hg ci -Ama -d'0 0'
# mkdir b
# echo a > b/a
# chmod +x b/a
# hg ci -Amb -d'1 0'

sh % "hg init"
sh % "hg -q pull '$TESTDIR/bundles/test-manifest.hg'"

# The next call is expected to return nothing:

sh % "hg manifest"

sh % "hg co" == "3 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg manifest" == r"""
    a
    b/a
    l"""

sh % "hg files -vr ." == r"""
             2   a
             2 x b/a
             1 l l"""
sh % "hg files -r . -X b" == r"""
    a
    l"""

sh % "hg manifest -v" == r"""
    644   a
    755 * b/a
    644 @ l"""

sh % "hg manifest --debug" == r"""
    b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a
    b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 755 * b/a
    047b75c6d7a3ef6a2243bd0e99f94f6ea6683597 644 @ l"""

sh % "hg manifest -r 0" == r"""
    a
    l"""

sh % "hg manifest -r 1" == r"""
    a
    b/a
    l"""

sh % "hg manifest -r tip" == r"""
    a
    b/a
    l"""

sh % "hg manifest tip" == r"""
    a
    b/a
    l"""

sh % "hg manifest --all" == r"""
    a
    b/a
    l"""

# The next two calls are expected to abort:

sh % "hg manifest -r 2" == r"""
    abort: unknown revision '2'!
    (if 2 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""

sh % "hg manifest -r tip tip" == r"""
    abort: please specify just one revision
    [255]"""
