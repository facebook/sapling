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

# Verify committing odd filenames works (with % character)

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a > 'bad%name'
  $ hg commit -Am badname
  adding bad%name
  $ echo b > 'bad%name'
  $ hg commit -Am badname2
