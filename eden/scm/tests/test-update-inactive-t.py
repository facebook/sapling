# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# hg update --inactive should behave like update except that
# it should not activate deactivated bookmarks and
# should not print the related ui.status outputs
# (eg: "activating bookmarks")

# Set up the repository.
sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag -m '+4 *3 +1'"
sh % "hg bookmark -r 7db39547e641 test"
sh % "hg update test" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark test)"""
sh % "hg bookmarks" == " * test                      5:7db39547e641"
sh % "hg bookmark -i test"
sh % "hg update --inactive test" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg bookmarks" == "   test                      5:7db39547e641"
sh % "hg bookmark -r 09bb8c08de89 test2"
sh % "hg update test" == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (activating bookmark test)"""
sh % "hg update --inactive test2" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark test)"""
sh % "hg bookmarks" == r"""
       test                      5:7db39547e641
       test2                     1:09bb8c08de89"""
sh % "hg update --inactive test" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg bookmarks" == r"""
       test                      5:7db39547e641
       test2                     1:09bb8c08de89"""
