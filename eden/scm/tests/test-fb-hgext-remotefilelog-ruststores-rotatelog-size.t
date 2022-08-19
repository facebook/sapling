#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ setconfig remotefilelog.useruststore=True remotefilelog.write-hgcache-to-indexedlog=True
  $ setconfig remotefilelog.debug=False

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow

  $ echo uuuuuuuuuuu > u
  $ hg commit -qAm u
  $ echo vvvvvvvvvvv > v
  $ hg commit -qAm v
  $ echo wwwwwwwwwww > w
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
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 12 *0* (glob)
  $ cd shallow2

  $ cp .hg/hgrc .hg/hgrc.bak
  $ setconfig indexedlog.data.max-bytes-per-log=10
  $ hg up -q 'desc(u)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 70 *0* (glob)
  * 12 *1* (glob)
  $ hg up -q 'desc(v)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 70 *0* (glob)
  * 70 *1* (glob)
  * 12 *2* (glob)
  $ hg up -q 'desc(w)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 70 *0* (glob)
  * 70 *1* (glob)
  * 70 *2* (glob)
  * 12 *3* (glob)

  $ setconfig indexedlog.data.max-bytes-per-log=100
  $ hg up -q null
  $ newcachedir

  $ hg up -q 'desc(u)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * * *0* (glob)
  $ hg up -q 'desc(v)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 128 *0* (glob)
  * 12 *1* (glob)
  $ hg up -q 'desc(w)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 128 *0* (glob)
  * 70 *1* (glob)

Test max-log-count
  $ hg up -q null
  $ newcachedir
  $ setconfig indexedlog.data.max-bytes-per-log=10 indexedlog.data.max-log-count=3
  $ hg up -q 'desc(u)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2
  $ hg up -q 'desc(v)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
  $ hg up -q 'desc(w)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
  $ hg up -q 'desc(x)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  3
- Verify the log shrinks at the next rotation when the max-log-count is reduced.
  $ setconfig indexedlog.data.max-log-count=2
  $ hg up -q 'desc(y)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2
  $ hg up -q 'desc(z)'
  $ findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log' | wc -l | sed -e 's/ //g'
  2

Test remotefilelog.cachelimit
  $ cp .hg/hgrc.bak .hg/hgrc
  $ hg up -q null
  $ newcachedir
  $ setconfig remotefilelog.cachelimit=300B
  $ hg up -q 'desc(u)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 70 *0* (glob)
  $ hg up -q 'desc(v)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 128 *0* (glob)
  * 12 *1* (glob)
  $ hg up -q 'desc(w)'
  $ ls_l $(findfilessorted "$CACHEDIR/master/" | grep -v 'manifest' | grep 'datastore.*log')
  * 128 *0* (glob)
  * 70 *1* (glob)
