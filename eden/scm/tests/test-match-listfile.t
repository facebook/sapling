#debugruntest-compatible

  $ configure modern

  $ newrepo

Empty listfile should not match everything.
  $ touch foo
  $ touch $TESTTMP/empty_listfile
  $ hg add listfile:$TESTTMP/empty_listfile
  empty listfile $TESTTMP/empty_listfile matches nothing
  $ hg status
  ? foo
