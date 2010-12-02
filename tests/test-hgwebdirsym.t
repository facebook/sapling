Tests whether or not hgwebdir properly handles various symlink topologies.

  $ "$TESTDIR/hghave" symlink || exit 80
  $ hg init a
  $ echo a > a/a
  $ hg --cwd a ci -Ama -d'1 0'
  adding a
  $ mkdir webdir
  $ cd webdir
  $ hg init b
  $ echo b > b/b
  $ hg --cwd b ci -Amb -d'2 0'
  adding b
  $ hg init c
  $ echo c > c/c
  $ hg --cwd c ci -Amc -d'3 0'
  adding c
  $ ln -s ../a al
  $ ln -s ../webdir circle
  $ root=`pwd`
  $ cd ..
  $ cat > collections.conf <<EOF
  > [collections]
  > $root=$root
  > EOF
  $ hg serve -p $HGPORT -d --pid-file=hg.pid --webdir-conf collections.conf \
  >     -A access-collections.log -E error-collections.log
  $ cat hg.pid >> $DAEMON_PIDS

should succeed

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/?style=raw'
  200 Script output follows
  
  
  /al/
  /b/
  /c/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/al/file/tip/a?style=raw'
  200 Script output follows
  
  a
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/b/file/tip/b?style=raw'
  200 Script output follows
  
  b
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/c/file/tip/c?style=raw'
  200 Script output follows
  
  c

should fail

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/circle/al/file/tip/a?style=raw'
  404 Not Found
  
  
  error: repository circle/al/file/tip/a not found
  [1]
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/circle/b/file/tip/a?style=raw'
  404 Not Found
  
  
  error: repository circle/b/file/tip/a not found
  [1]
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/circle/c/file/tip/a?style=raw'
  404 Not Found
  
  
  error: repository circle/c/file/tip/a not found
  [1]

collections errors

  $ cat error-collections.log
