#debugruntest-compatible

#require no-eden

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

Check that logging fails because of wrong permissions
  $ echo foo > foo
  $ hg commit -Aqm foo --debug --config infinitepushbackup.autobackup=true
  adding foo
  committing files:
  foo
  committing manifest
  committing changelog
  committed e63c23eaa88ae77967edcf4ea194d31167c478b0
  starting commit cloud autobackup in the background
  $TESTTMP/logs directory has incorrect permission, background backup logging will be disabled
  $ waitbgbackup
