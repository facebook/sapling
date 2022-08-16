#chg-compatible
#debugruntest-compatible

  $ configure modernclient

  $ newclientrepo repo
  $ echo a > a
  $ hg ci -Amq a

Test that we don't accidentally write non-readable files.
  $ find . -not -perm -0444
