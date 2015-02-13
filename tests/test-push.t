Set up extension and repos

  $ echo "[phases]" >> $HGRCPATH
  $ echo "publish = False" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=$(dirname $TESTDIR)/remotenames.py" >> $HGRCPATH
  $ hg init repo1
  $ hg clone repo1 repo2
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2

Test that anonymous heads are disallowed by default

  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg push
  pushing to $TESTTMP/repo1
  searching for changes
  abort: push would create new anonymous heads (cb9a9f314b8b)
  (use 'hg push --to NAME' to create a new remote bookmark)
  [255]

Create a remote bookmark

  $ hg push --to @ -f
  pushing rev cb9a9f314b8b to destination $TESTTMP/repo1 bookmark @
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  exporting bookmark @

Test that we can still push a head that advances a remote bookmark

  $ echo b >> a
  $ hg commit -m b
  $ hg book @
  $ hg push
  pushing to $TESTTMP/repo1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark @
