  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# commit a new version of x so we can gc the old one

  $ cd master
  $ echo y > x
  $ hg commit -qAm y
  $ cd ..

  $ cd shallow
  $ hg pull -q
  $ hg update -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cd ..

# gc client cache

  $ lastweek=`python -c 'import datetime,time; print datetime.datetime.fromtimestamp(time.time() - (86400 * 7)).strftime("%y%m%d%H%M")'`
  $ find $CACHEDIR -type f -exec touch -t $lastweek {} \;

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0 (glob)
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/48023ec064c1d522f0d792a5a912bb1bf7859a4a (glob)
  $TESTTMP/hgcache/repos (glob)
  $ hg gc
  finished: removed 1 of 2 files (0.00 GB to 0.00 GB)
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/48023ec064c1d522f0d792a5a912bb1bf7859a4a (glob)
  $TESTTMP/hgcache/repos

# gc server cache

  $ find master/.hg/remotefilelogcache -type f | sort
  master/.hg/remotefilelogcache/x/1406e74118627694268417491f018a4a883152f0 (glob)
  master/.hg/remotefilelogcache/x/48023ec064c1d522f0d792a5a912bb1bf7859a4a (glob)
  $ hg gc master
  finished: removed 0 of 1 files (0.00 GB to 0.00 GB)
  $ find master/.hg/remotefilelogcache -type f | sort
  master/.hg/remotefilelogcache/x/48023ec064c1d522f0d792a5a912bb1bf7859a4a (glob)

  $ cp $CACHEDIR/repos $CACHEDIR/repos.bak
  $ echo " " > $CACHEDIR/repos
  $ hg gc
  warning: no valid repos in repofile
  $ mv $CACHEDIR/repos.bak $CACHEDIR/repos


  $ printf "asdas\0das" >> $CACHEDIR/repos
  $ hg gc 2>&1 | head -n2
  warning: malformed path: * (glob)
  Traceback (most recent call last):
