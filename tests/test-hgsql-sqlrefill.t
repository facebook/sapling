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


Show that sqlrefill does not fix the server. This will be fixed in D8925895.

  $ hg sqlrefill --i-know-what-i-am-doing 0
  $ hg log -GT '{files}' 2>&1 | grep "CorruptionException:"
  hgext.hgsql.CorruptionException: heads don't match after sync
