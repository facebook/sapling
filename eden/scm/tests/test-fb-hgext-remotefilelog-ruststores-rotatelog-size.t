#chg-compatible

  $ . "$TESTDIR/library.sh"
  $ setconfig remotefilelog.useruststore=True remotefilelog.write-hgcache-to-indexedlog=True
  $ setconfig remotefilelog.debug=False

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow

  $ echo zzzzzzzzzzz > u
  $ hg commit -qAm u
  $ echo zzzzzzzzzzz > v
  $ hg commit -qAm v
  $ echo zzzzzzzzzzz > w
  $ hg commit -qAm w
  $ echo xxxxxxxxxxx > x
  $ hg commit -qAm x
  $ echo yyyyyyyyyyy > y
  $ hg commit -qAm y
  $ echo zzzzzzzzzzz > z
  $ hg commit -qAm z
  $ hg push -q -r tip --to master --create
  $ cd ..


Test max-bytes-per-log
  $ clone master shallow2 --noupdate
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--      12 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  $ cd shallow2

  $ setconfig indexedlog.data.max-bytes-per-log=10
  $ hg up -q 0
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  -rw-rw-r--      12 $TESTTMP/hgcache/master/indexedlogdatastore/1/log
  $ hg up -q 1
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/1/log
  -rw-rw-r--      12 $TESTTMP/hgcache/master/indexedlogdatastore/2/log
  $ hg up -q 2
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/1/log
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/2/log
  -rw-rw-r--      12 $TESTTMP/hgcache/master/indexedlogdatastore/3/log

  $ setconfig indexedlog.data.max-bytes-per-log=100
  $ hg up -q null
  $ clearcache

  $ hg up -q 0
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  $ hg up -q 1
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--     128 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  -rw-rw-r--      12 $TESTTMP/hgcache/master/indexedlogdatastore/1/log
  $ hg up -q 2
  $ ls_l $(findfilessorted $CACHEDIR/master/ | grep 'datastore.*log')
  -rw-rw-r--     128 $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  -rw-rw-r--      70 $TESTTMP/hgcache/master/indexedlogdatastore/1/log

Test max-log-count
  $ hg up -q null
  $ clearcache
  $ setconfig indexedlog.data.max-bytes-per-log=10 indexedlog.data.max-log-count=3
  $ hg up -q 0
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2
  $ hg up -q 1
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
  $ hg up -q 2
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
  $ hg up -q 3
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
- Verify the log shrinks at the next rotation when the max-log-count is reduced.
  $ setconfig indexedlog.data.max-log-count=2
  $ hg up -q 4
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2
  $ hg up -q 5
  $ findfilessorted $CACHEDIR/master/ | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2
