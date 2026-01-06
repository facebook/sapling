#require eden

Basic test demonstrating that ".hg" dir is now a symlink in eden mounts.

  $ newclientrepo repo
  $ ls -l .hg
  lrwx* .hg -> */eden/clients/repo/sl-repo-dir (glob)
  $ touch foo
  $ hg commit -Aqm foo
  $ hg push -q --to master --create
  $ hg log -G
  @  commit:      1f7b0de80e11
     bookmark:    remote/master
     hoistedname: master
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     foo

  $ cd
  $ eden rm repo 2>/dev/null
  Removing $TESTTMP/repo...
  Stopping aux processes for $TESTTMP/repo...
  Unmounting `$TESTTMP/repo`. Please be patient: this can take up to 1 minute!
  Deleting mount $TESTTMP/repo...
  Cleaning up mount $TESTTMP/repo...
  Success
