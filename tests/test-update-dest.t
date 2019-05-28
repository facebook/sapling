  $ setconfig extensions.treemanifest=!
Test update.requiredest
  $ cd $TESTTMP
  $ cat >> $HGRCPATH <<EOF
  > [commands]
  > update.requiredest = True
  > EOF
  $ hg init repo
  $ cd repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ hg up
  abort: you must specify a destination
  (for example: hg update ".::")
  [255]
  $ hg up .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGPLAIN=1 hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --config commands.update.requiredest=False up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..

Check update.requiredest interaction with pull --update
  $ hg clone repo clone
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ cd ../clone
  $ hg pull --update
  abort: update destination required by configuration
  (use hg pull followed by hg update DEST)
  [255]

  $ cd ..

update.requiredest should silent the "hg update" text after pull
  $ hg init repo1
  $ cd repo1
  $ hg pull ../repo
  pulling from ../repo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 8f0162e483d0:048c2cb95949
