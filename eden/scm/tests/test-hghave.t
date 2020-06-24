#chg-compatible

Testing that hghave does not crash when checking features

  $ hg debugpython -- $TESTDIR/hghave --test-features 2>/dev/null
