#testcases case-innodb case-rocksdb

#if case-rocksdb
  $ DBENGINE=rocksdb
#else
  $ DBENGINE=innodb
#endif

  $ . "$TESTDIR/hgsql/library.sh"


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
  hgext.hgsql.CorruptionException: heads don't match after sync


Fix the server using sqlrefill.

  $ hg sqlrefill --i-know-what-i-am-doing 0
  $ hg log -GT '{files}'
  @  r
  |
  | o  q
  |/
  o  p
  


Show that making a new commit to master fails. This will be fixed in D8925906.

  $ echo s > s
  $ hg commit -qAm s 2>&1 | grep "CorruptionException:"
  hgext.hgsql.CorruptionException: expected node )\x8d\xdd>\xb4x\x8dk\x1c\xde]\xf7\xd4\xc7Sc\xc2\xa5\xe0\xa7\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 at rev 2 of 00changelog.i but found 298ddd3eb4788d6b1cde5df7d4c75363c2a5e0a7 (esc)
