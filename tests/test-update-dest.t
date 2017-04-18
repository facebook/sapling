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
