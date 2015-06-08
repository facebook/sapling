This file tests the behavior of run-tests.py itself.

Avoid interference from actual test env:

  $ unset HGTEST_JOBS
  $ unset HGTEST_TIMEOUT
  $ unset HGTEST_PORT
  $ unset HGTEST_SHELL

Smoke test
============

  $ run-tests.py $HGTEST_RUN_TESTS_PURE
  
  # Ran 0 tests, 0 skipped, 0 warned, 0 failed.

a succesful test
=======================

  $ cat > test-success.t << EOF
  >   $ echo babar
  >   babar
  >   $ echo xyzzy
  >   never happens (?)
  >   xyzzy
  >   nor this (?)
  > EOF

  $ run-tests.py --with-hg=`which hg`
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

  >>> fh = open('test-failure-unicode.t', 'wb')
  >>> fh.write(u'  $ echo babar\u03b1\n'.encode('utf-8')) and None
  >>> fh.write(u'  l\u03b5\u03b5t\n'.encode('utf-8')) and None

  $ run-tests.py --with-hg=`which hg`
  
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
  --- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  
  ERROR: test-failure-unicode.t output changed
  !
  Failed test-failure.t: output changed
  Failed test-failure-unicode.t: output changed
  # Ran 3 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]

test --xunit support
  $ run-tests.py --with-hg=`which hg` --xunit=xunit.xml
  
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
  --- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  
  ERROR: test-failure-unicode.t output changed
  !
  Failed test-failure.t: output changed
  Failed test-failure-unicode.t: output changed
  # Ran 3 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="2" name="run-tests" skipped="0" tests="3">
    <testcase name="test-success.t" time="*"/> (glob)
    <testcase name="test-failure-unicode.t" time="*"> (glob)
  <![CDATA[--- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  ]]>  </testcase>
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

  $ rm test-failure-unicode.t

test for --retest
====================

  $ run-tests.py --with-hg=`which hg` --retest
  
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

  $ run-tests.py --with-hg=`which hg` test-success.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

success w/ keyword
  $ run-tests.py --with-hg=`which hg` -k xyzzy
  .
  # Ran 2 tests, 1 skipped, 0 warned, 0 failed.

failed

  $ run-tests.py --with-hg=`which hg` test-failure.t
  
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
  $ run-tests.py --with-hg=`which hg` -k rataxes
  
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

Verify that when a process fails to start we show a useful message
==================================================================
NOTE: there is currently a bug where this shows "2 failed" even though
it's actually the same test being reported for failure twice.

  $ cat > test-serve-fail.t <<EOF
  >   $ echo 'abort: child process failed to start blah'
  > EOF
  $ run-tests.py --with-hg=`which hg` test-serve-fail.t
  
  ERROR: test-serve-fail.t output changed
  !
  ERROR: test-serve-fail.t output changed
  !
  Failed test-serve-fail.t: server failed to start (HGPORT=*) (glob)
  Failed test-serve-fail.t: output changed
  # Ran 1 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]
  $ rm test-serve-fail.t

Running In Debug Mode
======================

  $ run-tests.py --with-hg=`which hg` --debug 2>&1 | grep -v pwd
  + echo *SALT* 0 0 (glob)
  *SALT* 0 0 (glob)
  + echo babar
  babar
  + echo *SALT* 4 0 (glob)
  *SALT* 4 0 (glob)
  .+ echo *SALT* 0 0 (glob)
  *SALT* 0 0 (glob)
  + echo babar
  babar
  + echo *SALT* 2 0 (glob)
  *SALT* 2 0 (glob)
  + echo xyzzy
  xyzzy
  + echo *SALT* 6 0 (glob)
  *SALT* 6 0 (glob)
  .
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

Parallel runs
==============

(duplicate the failing test to get predictable output)
  $ cp test-failure.t test-failure-copy.t

  $ run-tests.py --with-hg=`which hg` --jobs 2 test-failure*.t -n
  !!
  Failed test-failure*.t: output changed (glob)
  Failed test-failure*.t: output changed (glob)
  # Ran 2 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]

failures in parallel with --first should only print one failure
  >>> f = open('test-nothing.t', 'w')
  >>> f.write('foo\n' * 1024) and None
  >>> f.write('  $ sleep 1') and None
  $ run-tests.py --with-hg=`which hg` --jobs 2 --first
  
  --- $TESTTMP/test-failure*.t (glob)
  +++ $TESTTMP/test-failure*.t.err (glob)
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  Failed test-failure*.t: output changed (glob)
  Failed test-nothing.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]


(delete the duplicated test file)
  $ rm test-failure-copy.t test-nothing.t


Interactive run
===============

(backup the failing test)
  $ cp test-failure.t backup

Refuse the fix

  $ echo 'n' | run-tests.py --with-hg=`which hg` -i
  
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

Interactive with custom view

  $ echo 'n' | run-tests.py --with-hg=`which hg` -i --view echo
  $TESTTMP/test-failure.t $TESTTMP/test-failure.t.err (glob)
  Accept this change? [n]* (glob)
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

View the fix

  $ echo 'y' | run-tests.py --with-hg=`which hg` --view echo
  $TESTTMP/test-failure.t $TESTTMP/test-failure.t.err (glob)
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Accept the fix

  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/foo.hg" >> test-failure.t
  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/foo.hg (glob)" >> test-failure.t
  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/*.hg (glob)" >> test-failure.t
  $ echo 'y' | run-tests.py --with-hg=`which hg` -i 2>&1 | \
  >   sed -e 's,(glob)$,&<,g'
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,9 +1,9 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
  -  saved backup bundle to $TESTTMP/foo.hg
  +  saved backup bundle to $TESTTMP/foo.hg (glob)<
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
     saved backup bundle to $TESTTMP/foo.hg (glob)<
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
  Accept this change? [n] ..
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

  $ sed -e 's,(glob)$,&<,g' test-failure.t
    $ echo babar
    babar
  This is a noop statement so that
  this test is still more bytes than success.
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/foo.hg (glob)<
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/foo.hg (glob)<
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/*.hg (glob)<

(reinstall)
  $ mv backup test-failure.t

No Diff
===============

  $ run-tests.py --with-hg=`which hg` --nodiff
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

test for --time
==================

  $ run-tests.py --with-hg=`which hg` test-success.t --time
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  start   end     cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

test for --time with --job enabled
====================================

  $ run-tests.py --with-hg=`which hg` test-success.t --time --jobs 2
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  start   end     cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

Skips
================
  $ cat > test-skip.t <<EOF
  >   $ echo xyzzy
  > #require false
  > EOF
  $ run-tests.py --with-hg=`which hg` --nodiff
  !.s
  Skipped test-skip.t: skipped
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ run-tests.py --with-hg=`which hg` --keyword xyzzy
  .s
  Skipped test-skip.t: skipped
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.

Skips with xml
  $ run-tests.py --with-hg=`which hg` --keyword xyzzy \
  >  --xunit=xunit.xml
  .s
  Skipped test-skip.t: skipped
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="0" name="run-tests" skipped="2" tests="2">
    <testcase name="test-success.t" time="*"/> (glob)
  </testsuite>

Missing skips or blacklisted skips don't count as executed:
  $ echo test-failure.t > blacklist
  $ run-tests.py --with-hg=`which hg` --blacklist=blacklist \
  >   test-failure.t test-bogus.t
  ss
  Skipped test-bogus.t: Doesn't exist
  Skipped test-failure.t: blacklisted
  # Ran 0 tests, 2 skipped, 0 warned, 0 failed.

#if json

test for --json
==================

  $ run-tests.py --with-hg=`which hg` --json
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  
  ERROR: test-failure.t output changed
  !.s
  Skipped test-skip.t: skipped
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ cat report.json
  testreport ={
      "test-failure.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "failure", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-skip.t": {
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "skip", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-success.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }
  } (no-eol)

Test that failed test accepted through interactive are properly reported:

  $ cp test-failure.t backup
  $ echo y | run-tests.py --with-hg=`which hg` --json -i
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,4 +1,4 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
  Accept this change? [n] ..s
  Skipped test-skip.t: skipped
  # Ran 2 tests, 1 skipped, 0 warned, 0 failed.

  $ cat report.json
  testreport ={
      "test-failure.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-skip.t": {
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "skip", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-success.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }
  } (no-eol)
  $ mv backup test-failure.t

#endif

backslash on end of line with glob matching is handled properly

  $ cat > test-glob-backslash.t << EOF
  >   $ echo 'foo bar \\'
  >   foo * \ (glob)
  > EOF

  $ run-tests.py --with-hg=`which hg` test-glob-backslash.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

  $ rm -f test-glob-backslash.t

