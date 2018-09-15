#require killdaemons

  $ enable share

prepare repo1

  $ newrepo
  $ echo a > a
  $ hg commit -A -q -m "init"

make a bundle we will use later

  $ cd ..
  $ hg -R repo1 bundle -q -a testbundle.hg

share it without bookmarks

  $ hg share repo1 repo2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

share it with bookmarks

  $ hg share -B repo1 repo3
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

add a future feature to repo1

  $ echo test-futurestorefeature > repo1/.hg/requires

running log should fail because of the new store format feature

  $ hg -R repo1 log -T '{node}\n'
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

but it doesn't for the shared repositories

  $ hg -R repo2 log -T '{node}\n'
  d3873e73d99ef67873dac33fbcc66268d5d2b6f4
  $ hg -R repo3 log -T '{node}\n'
  d3873e73d99ef67873dac33fbcc66268d5d2b6f4

commands that lock the local working copy are ok, because wlock implies shared-wlock

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

but commands that only lock the store don't check the shared repo requirements

  $ hg -R repo1 unbundle testbundle.hg
  abort: repository requires features unknown to this Mercurial: test-futurestorefeature!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg -R repo2 unbundle testbundle.hg
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg -R repo3 unbundle testbundle.hg
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
  (run 'hg update' to get a working copy)
