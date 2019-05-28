  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ initserver master2 masterrepo
  $ cd master
  $ hg log
  $ hg pull -q ../client

# Verify local commits work

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo y > y
  $ hg commit -Am y
  adding y

  $ cd ../master2
  $ hg log -l 1
  changeset:   1:d34c38483be9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  

# Verify local bookmarking works

  $ hg bookmark -r 1 @
  $ hg log -r @ --template '{rev}\n'
  1
  $ cd ../master
  $ hg log -r @ --template '{rev}\n'
  1
