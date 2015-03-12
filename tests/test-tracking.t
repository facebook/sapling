Set up extension and repos

  $ echo "[phases]" >> $HGRCPATH
  $ echo "publish = False" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=`dirname $TESTDIR`/remotenames.py" >> $HGRCPATH
  $ hg init repo1

Make sure we don't fail when rebase doesn't exist

  $ hg rebase
  hg: unknown command 'rebase'
  'rebase' is provided by the following extension:
  
      rebase        command to move sets of revisions to a different ancestor
  
  (use "hg help extensions" for information on enabling extensions)
  [255]
  $ echo "rebase=" >> $HGRCPATH

Create a tracking bookmark

  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -m a1
  $ echo a >> a
  $ hg commit -m a2
  $ hg book a
  $ hg up ".^"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark a)
  $ echo b > b
  $ hg add b
  $ hg commit -m b
  created new head
  $ hg book b -t a
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  2 b b
  |
  | o  1 a2 a
  |/
  o  0 a1
  
  $ hg book -v
     a                         1:fdceb0e57656
   * b                         2:a36ba4057bfd             [a: 1 ahead, 1 behind]
  $ hg rebase
  rebasing 2:a36ba4057bfd "b" (tip b)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/a36ba4057bfd-8ec5973a-backup.hg (glob)
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}'
  @  2 b b
  |
  o  1 a2 a
  |
  o  0 a1
  
  $ hg book -v
     a                         1:fdceb0e57656
   * b                         2:01c5289520dd             [a: 1 ahead, 0 behind]

Test push tracking

  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  2 b  default/b
  |
  o  1 a2  default/a
  |
  o  0 a1
  

  $ hg boo c -t default/b
  $ echo c > c
  $ hg add c
  $ hg commit -m c
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c
  |
  o  2 b  default/b
  |
  o  1 a2  default/a
  |
  o  0 a1
  
  $ hg push
  pushing rev aff78bd8e592 to destination $TESTTMP/repo1 bookmark b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark b
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c default/b
  |
  o  2 b
  |
  o  1 a2  default/a
  |
  o  0 a1
  

Test that we don't try to push if tracking bookmark isn't a remote bookmark

  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "forceto = True" >> $HGRCPATH
  $ hg book c -t foo
  $ hg push
  abort: must specify --to when pushing
  (see configuration option remotenames.forceto)
  [255]

Test renaming a remote and tracking

  $ rm .hg/remotenames
  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "rename.default = remote" >> $HGRCPATH
  $ hg pull
  pulling from $TESTTMP/repo1 (glob)
  searching for changes
  no changes found
  $ hg book c -t remote/a
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c remote/b
  |
  o  2 b
  |
  o  1 a2  remote/a
  |
  o  0 a1
  
  $ hg push
  pushing rev aff78bd8e592 to destination $TESTTMP/repo1 bookmark a
  searching for changes
  no changes found
  updating bookmark a
  [1]
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c remote/a remote/b
  |
  o  2 b
  |
  o  1 a2
  |
  o  0 a1
  

