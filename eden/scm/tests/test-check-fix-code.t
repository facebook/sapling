#chg-compatible

#require version-control no-eden

  $ eagerepo
  $ cd "$TESTDIR"/..
  warning: no longer inside TESTTMP

  $ sl-source-files '**' > $TESTTMP/filelist
  $ PYTHONPATH= $PYTHON "$TESTDIR/../contrib/fix-code.py" --dry-run `cat $TESTTMP/filelist`
# In case the above list is not empty, run 'contrib/fix-code.py FILE...' to
# fix them.
