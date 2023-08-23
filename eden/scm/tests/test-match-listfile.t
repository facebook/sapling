#debugruntest-compatible

#testcases rust python

#if rust
  $ setconfig experimental.rustmatcher=true
#else
  $ setconfig experimental.rustmatcher=false
#endif

  $ configure modern

  $ newrepo

Empty listfile should not match everything.
  $ touch foo
  $ touch $TESTTMP/empty_listfile
  $ hg add listfile:$TESTTMP/empty_listfile
  *empty listfile $TESTTMP/empty_listfile matches nothing (glob)
  $ hg status
  ? foo
