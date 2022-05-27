#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#require killdaemons

  $ enable share

# prepare repo1

  $ newrepo
  $ echo a > a
  $ hg commit -A -q -m init

# make a bundle we will use later

  $ cd ..
  $ hg -R repo1 bundle -q -a testbundle.hg

# share it without bookmarks

  $ hg share repo1 repo2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# share it with bookmarks

  $ hg share -B repo1 repo3
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# add a future store feature to repo1

  $ echo test-futurestorefeature > repo1/.hg/store/requires

# running log should fail because of the new store format feature

  $ hg -R repo1 log -T '{node}\n'
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo2 log -T '{node}\n'
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo3 log -T '{node}\n'
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

# commands that lock the local working copy also fail correctly

  $ hg -R repo1 co 0
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo2 co 0
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo3 co 0
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

# commands that only lock the store also fail correctly

  $ hg -R repo1 unbundle testbundle.hg
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo2 unbundle testbundle.hg
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo3 unbundle testbundle.hg
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
