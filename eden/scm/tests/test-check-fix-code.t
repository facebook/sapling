#debugruntest-compatible
#chg-compatible

#require test-repo no-eden

  $ eagerepo
  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

  $ testrepohg files . > $TESTTMP/filelist
  $ PYTHONPATH= ./contrib/fix-code.py --dry-run `cat $TESTTMP/filelist`
# In case the above list is not empty, run 'contrib/fix-code.py FILE...' to
# fix them.
