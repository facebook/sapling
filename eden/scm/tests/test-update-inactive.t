#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hg goto --inactive should behave like update except that
# it should not activate deactivated bookmarks and
# should not print the related ui.status outputs
# (eg: "activating bookmarks")
# Set up the repository.

  $ hg init repo
  $ cd repo
  $ hg debugbuilddag -m '+4 *3 +1'
  $ hg bookmark -r 7db39547e641 test
  $ hg goto test
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test)
  $ hg bookmarks
   * test                      7db39547e641
  $ hg bookmark -i test
  $ hg goto --inactive test
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmarks
     test                      7db39547e641
  $ hg bookmark -r 09bb8c08de89 test2
  $ hg goto test
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test)
  $ hg goto --inactive test2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ hg bookmarks
     test                      7db39547e641
     test2                     09bb8c08de89
  $ hg goto --inactive test
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmarks
     test                      7db39547e641
     test2                     09bb8c08de89
