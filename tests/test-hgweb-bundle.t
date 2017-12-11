#require serve

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > strip=
  > EOF

  $ echo 1 > foo
  $ hg commit -A -m 'first'
  adding foo
  $ echo 2 > bar
  $ hg commit -A -m 'second'
  adding bar

Produce a bundle to use

  $ hg strip -r 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/server/.hg/strip-backup/ed602e697e0f-cc9fff6a-backup.hg

Serve from a bundle file

  $ hg serve -R .hg/strip-backup/ed602e697e0f-cc9fff6a-backup.hg -d -p $HGPORT --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS

Ensure we're serving from the bundle

  $ (get-with-headers.py localhost:$HGPORT 'file/tip/?style=raw')
  200 Script output follows
  
  
  -rw-r--r-- 2 bar
  -rw-r--r-- 2 foo
  
  
