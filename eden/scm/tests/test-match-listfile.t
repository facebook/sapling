#debugruntest-compatible

  $ configure modern

  $ newrepo

Empty listfile should not match everything.
  $ touch foo
  $ touch $TESTTMP/empty_listfile
  $ hg add listfile:$TESTTMP/empty_listfile
  $ hg status
  ? foo
