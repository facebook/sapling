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

failing test
==================

  $ cat > test-failure.t << EOF
  >   $ echo babar
  >   rataxes
  > EOF

  $ $TESTDIR/run-tests.py --with-hg=`which hg`
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,2 +1,2 @@
     $ echo babar
  -  rataxes
  +  babar
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

test for --retest
====================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --retest
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,2 +1,2 @@
     $ echo babar
  -  rataxes
  +  babar
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 1 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Selecting Tests To Run
======================

successful

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-success.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

failed

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-failure.t
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,2 +1,2 @@
     $ echo babar
  -  rataxes
  +  babar
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Running In Debug Mode
======================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --debug
  + echo SALT* 0 0 (glob)
  SALT* 0 0 (glob)
  + echo babar
  babar
  + echo SALT* 2 0 (glob)
  SALT* 2 0 (glob)
  .+ echo SALT* 0 0 (glob)
  SALT* 0 0 (glob)
  + echo babar
  babar
  + echo SALT* 2 0 (glob)
  SALT* 2 0 (glob)
  .
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.
