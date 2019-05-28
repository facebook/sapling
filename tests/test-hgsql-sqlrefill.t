  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

Populate the db with an initial commit.

  $ initclient client
  $ cd client
  $ echo p > p
  $ hg commit -qAm p
  $ echo q > q
  $ hg commit -qAm q
  $ hg -q up 0
  $ echo r > r
  $ hg commit -qAm r
  $ hg bookmark foo
  $ cd ..


Create master without sql configuration.

  $ hg clone -q client master


Configure master as a server backed by sql.

  $ configureserver master masterrepo
  $ cd master
  $ hg log -GT '{files}' 2>&1 | grep "CorruptionException:"
  edenscm.hgext.hgsql.CorruptionException: heads don't match after sync


Fix the server using sqlrefill.

  $ hg sqlrefill --i-know-what-i-am-doing 0
  $ hg log -GT '{files}'
  @  r
  |
  | o  q
  |/
  o  p
  


Make another commit to the server to verify that repository state is sane after
the refill.

  $ echo s > s
  $ hg commit -qAm s
  $ hg log -GT '{files}'
  @  s
  |
  o  r
  |
  | o  q
  |/
  o  p
  
