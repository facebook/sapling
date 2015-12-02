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

  $ find $CACHEDIR -type f | wc -l | sed -e 's/ //g'
  3
  $ hg gc
  finished: removed 1 of 2 files (0.00 GB to 0.00 GB)
  $ find $CACHEDIR -type f | wc -l | sed -e 's/ //g'
  2

# gc server cache

  $ find master/.hg/remotefilelogcache -type f | wc -l | sed -e 's/ //g'
  2
  $ hg gc master
  finished: removed 0 of 1 files (0.00 GB to 0.00 GB)
  $ find master/.hg/remotefilelogcache -type f | wc -l | sed -e 's/ //g'
  1

  $ cp $CACHEDIR/repos $CACHEDIR/repos.bak
  $ echo " " > $CACHEDIR/repos
  $ hg gc
  warning: no valid repos in repofile
  $ mv $CACHEDIR/repos.bak $CACHEDIR/repos


  $ echo "asdas\0das" >> $CACHEDIR/repos
  $ hg gc
  warning: malformed path: 'asdas\x00das':must be encoded string without NULL bytes, not str
  Traceback (most recent call last):
    File * (glob)
      path = ui.expandpath(os.path.normpath(path))
    File * (glob)
      p = self.paths.getpath(loc)
    File * (glob)
      return path(None, rawloc=name)
    File * (glob)
      if not name and not u.scheme and not self._isvalidlocalpath(self.loc):
    File * (glob)
      return os.path.isdir(os.path.join(path, '.hg'))
    File * (glob)
      st = os.stat(s)
  TypeError: must be encoded string without NULL bytes, not str
  finished: removed 0 of 1 files (0.00 GB to 0.00 GB)
