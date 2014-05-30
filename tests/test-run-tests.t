This file tests the behavior of run-tests.py itself.

Smoke test
============

  $ $TESTDIR/run-tests.py
  
  # Ran 0 tests, 0 skipped, 0 warned, 0 failed.

a succesful test
=======================

  $ cat > test-success.t << EOF
  >   $ echo babar
  >   babar
  > EOF

  $ $TESTDIR/run-tests.py --with-hg=`which hg`
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
