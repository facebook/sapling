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
  >   $ echo xyzzy
  >   xyzzy
  > EOF

  $ $TESTDIR/run-tests.py --with-hg=`which hg`
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

failing test
==================

  $ cat > test-failure.t << EOF
  >   $ echo babar
  >   rataxes
  > This is a noop statement so that
  > this test is still more bytes than success.
  > EOF

  $ $TESTDIR/run-tests.py --with-hg=`which hg`
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
test --xunit support
  $ $TESTDIR/run-tests.py --with-hg=`which hg` --xunit=xunit.xml
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="1" name="run-tests" skipped="0" tests="2">
    <testcase name="test-success.t" time="*"/> (glob)
    <testcase name="test-failure.t" time="*"> (glob)
  <![CDATA[--- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  ]]>  </testcase>
  </testsuite>

test for --retest
====================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --retest
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Selecting Tests To Run
======================

successful

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-success.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

success w/ keyword
  $ $TESTDIR/run-tests.py --with-hg=`which hg` -k xyzzy
  .
  # Ran 2 tests, 1 skipped, 0 warned, 0 failed.

failed

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-failure.t
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

failure w/ keyword
  $ $TESTDIR/run-tests.py --with-hg=`which hg` -k rataxes
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Running In Debug Mode
======================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --debug | grep -v pwd
  + echo SALT* 0 0 (glob)
  SALT* 0 0 (glob)
  + echo babar
  babar
  + echo SALT* 4 0 (glob)
  SALT* 4 0 (glob)
  .+ echo SALT* 0 0 (glob)
  SALT* 0 0 (glob)
  + echo babar
  babar
  + echo SALT* 2 0 (glob)
  SALT* 2 0 (glob)
  + echo xyzzy
  xyzzy
  + echo SALT* 4 0 (glob)
  SALT* 4 0 (glob)
  .
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

Parallel runs
==============

(duplicate the failing test to get predictable output)
  $ cp test-failure.t test-failure-copy.t

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --jobs 2 test-failure*.t
  
  --- $TESTTMP/test-failure*.t (glob)
  +++ $TESTTMP/test-failure*.t.err (glob)
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure*.t output changed (glob)
  !
  --- $TESTTMP/test-failure*.t (glob)
  +++ $TESTTMP/test-failure*.t.err (glob)
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure*.t output changed (glob)
  !
  Failed test-failure*.t: output changed (glob)
  Failed test-failure*.t: output changed (glob)
  # Ran 2 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]

(delete the duplicated test file)
  $ rm test-failure-copy.t


Interactive run
===============

(backup the failing test)
  $ cp test-failure.t backup

Refuse the fix

  $ echo 'n' | $TESTDIR/run-tests.py --with-hg=`which hg` -i
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  Accept this change? [n] 
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ cat test-failure.t
    $ echo babar
    rataxes
  This is a noop statement so that
  this test is still more bytes than success.

View the fix

  $ echo 'y' | $TESTDIR/run-tests.py --with-hg=`which hg` --view echo
  $TESTTMP/test-failure.t $TESTTMP/test-failure.t.err
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Accept the fix

  $ echo 'y' | $TESTDIR/run-tests.py --with-hg=`which hg` -i
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  Accept this change? [n] ..
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

  $ cat test-failure.t
    $ echo babar
    babar
  This is a noop statement so that
  this test is still more bytes than success.

(reinstall)
  $ mv backup test-failure.t

No Diff
===============

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --nodiff
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

test for --time
==================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-success.t --time
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

test for --time with --job enabled
====================================

  $ $TESTDIR/run-tests.py --with-hg=`which hg` test-success.t --time --jobs 2
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

Skips
================
  $ cat > test-skip.t <<EOF
  >   $ echo xyzzy
  > #require false
  > EOF
  $ $TESTDIR/run-tests.py --with-hg=`which hg` --nodiff
  !.s
  Skipped test-skip.t: irrelevant
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ $TESTDIR/run-tests.py --with-hg=`which hg` --keyword xyzzy
  .s
  Skipped test-skip.t: irrelevant
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.

Skips with xml
  $ $TESTDIR/run-tests.py --with-hg=`which hg` --keyword xyzzy \
  >  --xunit=xunit.xml
  .s
  Skipped test-skip.t: irrelevant
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="0" name="run-tests" skipped="2" tests="2">
    <testcase name="test-success.t" time="*"/> (glob)
  </testsuite>

Missing skips or blacklisted skips don't count as executed:
  $ echo test-failure.t > blacklist
  $ $TESTDIR/run-tests.py --with-hg=`which hg` --blacklist=blacklist \
  >   test-failure.t test-bogus.t
  ss
  Skipped test-bogus.t: Doesn't exist
  Skipped test-failure.t: blacklisted
  # Ran 0 tests, 2 skipped, 0 warned, 0 failed.

