#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True


  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ newserver repo
  $ cd ..

Clone
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Create log dir
  $ mkdir $TESTTMP/logs

Setup background backup logging
  $ setconfig infinitepushbackup.logdir=$TESTTMP/logs
  $ mkcommit first

Check that logging fails because of wrong permissions
  $ hg cloud backup --background
  $ waitbgbackup
  $ hg cloud backup --background --debug
  $TESTTMP/logs directory has incorrect permission, background backup logging will be disabled
  $ waitbgbackup
