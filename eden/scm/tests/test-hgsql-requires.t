  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ cd master
  $ hg log
  $ hg pull -q ../client

Test that hgsql is a requirement
  $ grep hgsql .hg/requires
  hgsql
  $ hg log -r tip --config extensions.hgsql=!
  abort: repository requires features unknown to this Mercurial: hgsql!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ hg log -r tip
  changeset:   0:b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  

Ensure streaming clones to non-hgsql repos work
  $ cd ..
  $ hg clone --config extensions.hgsql=! --config ui.ssh='python "$TESTDIR/dummyssh"' --uncompressed ssh://user@dummy/master client2 | grep "streaming all changes"
  streaming all changes

Ensure streaming clones to hgsql repos work
  $ hg clone --config extensions.hgsql= --config ui.ssh='python "$TESTDIR/dummyssh"' --uncompressed ssh://user@dummy/master client3
  streaming all changes
  4 files to transfer, 294 bytes of data
  transferred 294 bytes in * seconds (*) (glob)
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
