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
  $ hg up .^
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
  
  $ hg rebase
  rebasing 2:a36ba4057bfd "b" (tip b)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/a36ba4057bfd-8ec5973a-backup.hg (glob)
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  2 b b
  |
  o  1 a2 a
  |
  o  0 a1
  
