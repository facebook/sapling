#chg-compatible
#debugruntest-compatible

#require execbit

  $ setconfig workingcopy.ruststatus=False
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am'not executable'
  adding a

  $ chmod +x a
  $ hg ci -m'executable'
  $ hg id
  79abf14474dc

Make sure we notice the change of mode if the cached size == -1:

  $ hg rm a
  $ hg revert -r d69afc33ff8a77eda0ccb79374772831912446c3 a
  $ hg debugstate
  n   0         -1 unset               a
  $ hg status
  M a

  $ hg up d69afc33ff8a77eda0ccb79374772831912446c3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  d69afc33ff8a
  $ test -x a && echo executable -- bad || echo not executable -- good
  not executable -- good

