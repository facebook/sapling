
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

Check that logging fails because of wrong permissions
  $ hg pushbackup --background
  $ waitbgbackup
  $ hg pushbackup --background --debug
  $TESTTMP/logs directory has incorrect permission, infinitepush backup logging will be disabled
  $ waitbgbackup
