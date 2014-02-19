  $ . "$TESTDIR/library.sh"

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

  $ cd ..

# Verify local pushes work

  $ cd client
  $ echo y > y
  $ hg commit -qAm y
  $ hg push ../master --traceback
  pushing to ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

# Verify local pulls work
  $ hg strip -q -r tip
  $ hg pull ../master
  pulling from ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log --template '{rev} {desc}\n'
  1 y
  0 x

# Verify local bookmark pull

  $ cd ../master
  $ hg book foo -r 0
  $ hg book
     foo                       0:b292c1e3311f
  $ cd ../client
  $ hg pull -q ../master
  $ hg book
     foo                       0:b292c1e3311f

# Verify local bookmark push

  $ hg book -r tip foo
  moving bookmark 'foo' forward from b292c1e3311f
  $ hg push ../master
  pushing to ../master
  searching for changes
  no changes found
  updating bookmark foo
  [1]
  $ hg book -R ../master
     foo                       1:d34c38483be9
