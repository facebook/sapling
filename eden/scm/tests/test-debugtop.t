#chg-compatible
#debugruntest-compatible

  $ enable progress
  $ setconfig extensions.rustprogresstest="$TESTDIR/runlogtest.py" runlog.enable=True runlog.progress_refresh=0
  $ newrepo

Make sure the table has the expected format
  $ hg debugtop -r 50000 -c "PROGRESS,TIME SPENT,CMD" | cat
  +----------+------------+----------------------------------------------+
  | PROGRESS | TIME SPENT | CMD                                          |
  +======================================================================+
  | -        | * | debugtop -r 50000 -c PROGRESS,TIME SPENT,CMD | (glob)
  +----------+------------+----------------------------------------------+

Test non-valid columns
  $ hg debugtop -c "not a valid column, not a valid column either"
  Error: column "not a valid column" was not expected
  Error: column "not a valid column either" was not expected
  [22]
