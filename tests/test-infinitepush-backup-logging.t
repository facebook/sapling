
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Clone
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Create log dir
  $ mkdir $TESTTMP/logs

Setup infinitepush backup logging
  $ printf "\n[infinitepushbackup]\nlogdir=$TESTTMP/logs" >> .hg/hgrc
  $ mkcommit first
  $ hg pushbackup --background
  $ waitbgbackup
  $ ls $TESTTMP/logs/test
  client\d{8} (re)

Set maxlognumber to 1, create a few fake log files and run pushbackup. Make sure
outdated files are deleted
  $ printf "\n[infinitepushbackup]\nmaxlognumber=1" >> .hg/hgrc
  $ touch $TESTTMP/logs/test/client19700101
  $ ls $TESTTMP/logs/test
  client\d{8} (re)
  client\d{8} (re)
  $ hg pushbackup --background
  $ waitbgbackup
  $ ls $TESTTMP/logs/test
  client\d{8} (re)
