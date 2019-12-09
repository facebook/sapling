#chg-compatible

  $ setconfig extensions.treemanifest=!
  > echo "[extensions]" >> $HGRCPATH
  > echo "remotenames=" >> $HGRCPATH
  > echo "[remotenames]" >> $HGRCPATH
  > echo "rename.default = remote" >> $HGRCPATH
  > echo "disallowedto = ^remote/" >> $HGRCPATH

Init the original "remote" repo

  $ hg init orig
  $ cd orig
  $ echo something > something
  $ hg ci -Am something
  adding something
  $ hg bookmark ababagalamaga
  $ cd ..

Clone original repo

  $ hg clone orig cloned
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned
  $ echo somethingelse > something
  $ hg ci -m somethingelse

Try to do the wrong push

  $ hg push --to remote/ababagalamaga
  pushing rev 71b4c8f22183 to destination $TESTTMP/orig bookmark remote/ababagalamaga
  abort: this remote bookmark name is not allowed
  (use another bookmark name)
  [255]

Try to do the right push

  $ hg push --to ababagalamaga
  pushing rev 71b4c8f22183 to destination $TESTTMP/orig bookmark ababagalamaga
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark ababagalamaga

Set up an svn default push path and test behavior

  $ hg paths --add default-push svn+ssh://nowhere/in/particular
  $ hg push --to foo ../orig
  pushing rev 71b4c8f22183 to destination ../orig bookmark foo
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]

